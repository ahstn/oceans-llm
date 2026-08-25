use std::io::{self, Write};

use serde::Serialize;
use serde_json::{Value, json};

use crate::redaction::MAX_INLINE_REQUEST_BYTES;

const STRATEGY_VERSION: &str = "structured-request-v2";
const ESSENTIAL_ENVELOPE_TARGET_BYTES: usize = 16 * 1024;
const MIN_ORDINARY_MESSAGE_BYTES: usize = 8 * 1024;
const MAX_ORDINARY_MESSAGE_BYTES: usize = 16 * 1024;
const MAX_SOLITARY_MESSAGE_BYTES: usize = 32 * 1024;
const MAX_TOTAL_CONTENT_BYTES: usize = 96 * 1024;
const MAX_AFFECTED_PATHS: usize = 32;
const TOOL_DESCRIPTION_BYTES: usize = 128;
const OMITTED_TOOL_VALUE: &str = "[omitted by gateway storage bound]";

#[derive(Debug)]
struct ContentLeaf {
    pointer: String,
    serialized_size: usize,
}

#[derive(Debug)]
struct ContentItem {
    leaves: Vec<ContentLeaf>,
    max_retained_bytes: usize,
}

#[derive(Debug, Default)]
struct BoundingFacts {
    affected_paths: Vec<String>,
    truncated_field_count: usize,
    tool_fields_compacted: usize,
    known_large_fields_truncated: usize,
}

impl BoundingFacts {
    fn record(&mut self, pointer: &str) {
        self.truncated_field_count = self.truncated_field_count.saturating_add(1);
        if self.affected_paths.len() < MAX_AFFECTED_PATHS {
            self.affected_paths.push(pointer.to_string());
        }
    }
}

#[derive(Default)]
struct CountingWriter {
    bytes: usize,
}

impl Write for CountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len());
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

#[derive(Default)]
struct PrefixCountingWriter {
    bytes: usize,
    prefix: Vec<u8>,
    prefix_limit: usize,
}

impl PrefixCountingWriter {
    fn new(prefix_limit: usize) -> Self {
        Self {
            prefix: Vec::with_capacity(prefix_limit),
            prefix_limit,
            ..Self::default()
        }
    }
}

impl Write for PrefixCountingWriter {
    fn write(&mut self, buffer: &[u8]) -> io::Result<usize> {
        self.bytes = self.bytes.saturating_add(buffer.len());
        let retained = buffer
            .len()
            .min(self.prefix_limit.saturating_sub(self.prefix.len()));
        self.prefix.extend_from_slice(&buffer[..retained]);
        Ok(buffer.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        Ok(())
    }
}

pub(crate) fn serialized_size<T>(value: &T) -> Result<usize, serde_json::Error>
where
    T: Serialize + ?Sized,
{
    let mut writer = CountingWriter::default();
    serde_json::to_writer(&mut writer, value)?;
    Ok(writer.bytes)
}

pub(crate) fn bound_request_payload(value: Value, max_bytes: usize) -> (Value, bool) {
    bound_request_payload_after_known_fields(value, max_bytes, None, 0)
}

pub(crate) fn bound_request_payload_after_known_fields(
    mut value: Value,
    max_bytes: usize,
    original_size: Option<usize>,
    known_large_field_count: usize,
) -> (Value, bool) {
    let max_bytes = max_bytes.min(MAX_INLINE_REQUEST_BYTES);
    let Ok(current_size) = serialized_size(&value) else {
        return serialization_failure_marker(max_bytes);
    };
    let original_size = original_size.unwrap_or(current_size);
    if current_size <= max_bytes && known_large_field_count == 0 {
        return (value, false);
    }

    let mut facts = BoundingFacts {
        truncated_field_count: known_large_field_count,
        known_large_fields_truncated: known_large_field_count,
        ..BoundingFacts::default()
    };
    if current_size <= max_bytes
        && try_attach_truncation_metadata(&mut value, original_size, max_bytes, &facts)
    {
        return (value, true);
    }
    let Some(items) = content_items(&value, max_bytes) else {
        return hard_fallback(value, original_size, max_bytes);
    };
    if essential_envelope_size(current_size, &items) > ESSENTIAL_ENVELOPE_TARGET_BYTES {
        compact_tool_envelope(&mut value, &mut facts);
    }

    let Ok(bounded_size) = serialized_size(&value) else {
        return serialization_failure_marker(max_bytes);
    };
    if facts.truncated_field_count > 0
        && bounded_size <= max_bytes
        && try_attach_truncation_metadata(&mut value, original_size, max_bytes, &facts)
    {
        return (value, true);
    }

    let metadata_overhead = truncation_metadata_overhead(original_size, max_bytes, &items, &facts);
    let available = content_replacement_budget(bounded_size, max_bytes, metadata_overhead, &items)
        .min(MAX_TOTAL_CONTENT_BYTES);

    if !content_items_fit_at_zero(&value, &items, available) {
        return hard_fallback(value, original_size, max_bytes);
    }

    let per_item_bytes = largest_content_budget(&value, &items, available);
    let fields_before_content = facts.truncated_field_count;
    truncate_content_items(&mut value, &items, per_item_bytes, &mut facts);
    if facts.truncated_field_count == fields_before_content {
        compact_tool_envelope(&mut value, &mut facts);
    }
    if facts.truncated_field_count == 0 {
        return hard_fallback(value, original_size, max_bytes);
    }

    insert_truncation_metadata(&mut value, original_size, max_bytes, &facts);
    match serialized_size(&value) {
        Ok(size) if size <= max_bytes => (value, true),
        _ => hard_fallback(value, original_size, max_bytes),
    }
}

fn content_replacement_budget(
    current_size: usize,
    max_bytes: usize,
    metadata_overhead: usize,
    items: &[ContentItem],
) -> usize {
    let existing_content = serialized_content_size(items);
    let envelope = current_size.saturating_sub(existing_content);
    max_bytes.saturating_sub(envelope.saturating_add(metadata_overhead))
}

fn serialized_content_size(items: &[ContentItem]) -> usize {
    items.iter().fold(0_usize, |total, item| {
        item.leaves.iter().fold(total, |total, leaf| {
            total.saturating_add(leaf.serialized_size)
        })
    })
}

fn essential_envelope_size(current_size: usize, items: &[ContentItem]) -> usize {
    current_size.saturating_sub(serialized_content_size(items))
}

fn content_items_fit_at_zero(value: &Value, items: &[ContentItem], available: usize) -> bool {
    replacement_size(value, items, 0) <= available
}

fn largest_content_budget(value: &Value, items: &[ContentItem], available: usize) -> usize {
    let mut low = 0_usize;
    let mut high = target_item_budget(items, available);
    while low < high {
        let candidate = low + (high - low).div_ceil(2);
        if replacement_size(value, items, candidate) <= available {
            low = candidate;
        } else {
            high = candidate.saturating_sub(1);
        }
    }
    low
}

fn target_item_budget(items: &[ContentItem], available: usize) -> usize {
    if items.is_empty() {
        return 0;
    }
    if items.len() == 1
        || items
            .iter()
            .any(|item| item.max_retained_bytes > MAX_ORDINARY_MESSAGE_BYTES)
    {
        return available.min(MAX_SOLITARY_MESSAGE_BYTES);
    }
    available
        .checked_div(items.len())
        .unwrap_or(0)
        .clamp(MIN_ORDINARY_MESSAGE_BYTES, MAX_ORDINARY_MESSAGE_BYTES)
}

fn replacement_size(value: &Value, items: &[ContentItem], per_item_bytes: usize) -> usize {
    items.iter().fold(0_usize, |total, item| {
        let item_bytes = per_item_bytes.min(item.max_retained_bytes);
        let per_leaf_bytes = item_bytes.checked_div(item.leaves.len()).unwrap_or(0);
        item.leaves.iter().fold(total, |total, leaf| {
            let size = value
                .pointer(&leaf.pointer)
                .and_then(Value::as_str)
                .map_or(0, |text| {
                    truncated_text(text, per_leaf_bytes).map_or(leaf.serialized_size, |truncated| {
                        serialized_string_size(&truncated)
                    })
                });
            total.saturating_add(size)
        })
    })
}

fn truncate_content_items(
    value: &mut Value,
    items: &[ContentItem],
    per_item_bytes: usize,
    facts: &mut BoundingFacts,
) {
    for item in items {
        let item_bytes = per_item_bytes.min(item.max_retained_bytes);
        let per_leaf_bytes = item_bytes.checked_div(item.leaves.len()).unwrap_or(0);
        for leaf in &item.leaves {
            let Some(text) = value.pointer(&leaf.pointer).and_then(Value::as_str) else {
                continue;
            };
            let Some(truncated) = truncated_text(text, per_leaf_bytes) else {
                continue;
            };
            if let Some(target) = value.pointer_mut(&leaf.pointer) {
                *target = Value::String(truncated);
                facts.record(&leaf.pointer);
            }
        }
    }
}

fn truncated_text(text: &str, retained_bytes: usize) -> Option<String> {
    if text.len() <= retained_bytes {
        return None;
    }
    let head_budget = retained_bytes.div_ceil(2);
    let tail_budget = retained_bytes / 2;
    let head_end = floor_char_boundary(text, head_budget);
    let tail_start = ceil_char_boundary(text, text.len().saturating_sub(tail_budget));
    Some(format!(
        "{}\n...[gateway truncated {} UTF-8 bytes]...\n{}",
        &text[..head_end],
        text.len()
            .saturating_sub(head_end + text.len().saturating_sub(tail_start)),
        &text[tail_start..]
    ))
}

fn floor_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while !text.is_char_boundary(index) {
        index = index.saturating_sub(1);
    }
    index
}

fn ceil_char_boundary(text: &str, mut index: usize) -> usize {
    index = index.min(text.len());
    while index < text.len() && !text.is_char_boundary(index) {
        index = index.saturating_add(1);
    }
    index
}

fn serialized_string_size(text: &str) -> usize {
    serialized_size(text).unwrap_or(usize::MAX)
}

fn content_items(value: &Value, max_bytes: usize) -> Option<Vec<ContentItem>> {
    let mut content_items = Vec::new();
    if let Some(instructions) = value.pointer("/body/instructions").and_then(Value::as_str) {
        content_items.push(ContentItem {
            leaves: vec![ContentLeaf {
                pointer: "/body/instructions".to_string(),
                serialized_size: serialized_string_size(instructions),
            }],
            max_retained_bytes: MAX_SOLITARY_MESSAGE_BYTES,
        });
    }
    if let Some(input) = value.pointer("/body/input").and_then(Value::as_str) {
        content_items.push(ContentItem {
            leaves: vec![ContentLeaf {
                pointer: "/body/input".to_string(),
                serialized_size: serialized_string_size(input),
            }],
            max_retained_bytes: MAX_SOLITARY_MESSAGE_BYTES,
        });
    }
    for collection in ["messages", "input"] {
        let Some(items) = value
            .pointer(&format!("/body/{collection}"))
            .and_then(Value::as_array)
        else {
            continue;
        };
        if collection == "input" && items.iter().all(Value::is_string) {
            if minimum_string_array_size(items.len()) > max_bytes {
                return None;
            }
            let mut leaves = Vec::with_capacity(items.len());
            for (index, item) in items.iter().enumerate() {
                let text = item.as_str().expect("input items were checked as strings");
                leaves.push(ContentLeaf {
                    pointer: format!("/body/input/{index}"),
                    serialized_size: serialized_string_size(text),
                });
            }
            if !leaves.is_empty() {
                content_items.push(ContentItem {
                    leaves,
                    max_retained_bytes: MAX_TOTAL_CONTENT_BYTES,
                });
            }
            continue;
        }
        for (index, item) in items.iter().enumerate() {
            let content_field = if let Some(content) = item.get("content") {
                Some(("content", content))
            } else if item.get("type").and_then(Value::as_str) == Some("function_call_output") {
                item.get("output").map(|output| ("output", output))
            } else {
                None
            };
            let Some((field, content)) = content_field else {
                continue;
            };
            let mut leaves = Vec::new();
            collect_content_leaves(
                content,
                &format!("/body/{collection}/{index}/{field}"),
                None,
                &mut leaves,
            );
            if !leaves.is_empty() {
                let important = matches!(
                    item.get("role").and_then(Value::as_str),
                    Some("system" | "developer")
                );
                content_items.push(ContentItem {
                    leaves,
                    max_retained_bytes: if important {
                        MAX_SOLITARY_MESSAGE_BYTES
                    } else {
                        MAX_ORDINARY_MESSAGE_BYTES
                    },
                });
            }
        }
    }
    if content_items.len() == 1 {
        content_items[0].max_retained_bytes = MAX_SOLITARY_MESSAGE_BYTES;
    }
    Some(content_items)
}

fn minimum_string_array_size(item_count: usize) -> usize {
    item_count.saturating_mul(3).saturating_add(1)
}

fn collect_content_leaves(
    value: &Value,
    pointer: &str,
    field_name: Option<&str>,
    leaves: &mut Vec<ContentLeaf>,
) {
    match value {
        Value::String(text) if field_name.is_none_or(is_bulky_content_field) => {
            leaves.push(ContentLeaf {
                pointer: pointer.to_string(),
                serialized_size: serialized_string_size(text),
            });
        }
        Value::Array(values) => {
            for (index, value) in values.iter().enumerate() {
                collect_content_leaves(value, &format!("{pointer}/{index}"), None, leaves);
            }
        }
        Value::Object(values) => {
            for (key, value) in values {
                collect_content_leaves(
                    value,
                    &format!("{pointer}/{}", escape_pointer_segment(key)),
                    Some(key),
                    leaves,
                );
            }
        }
        _ => {}
    }
}

fn is_bulky_content_field(field: &str) -> bool {
    matches!(
        field,
        "text"
            | "data"
            | "file_data"
            | "image_url"
            | "video_url"
            | "audio"
            | "input_text"
            | "output_text"
    )
}

fn escape_pointer_segment(value: &str) -> String {
    value.replace('~', "~0").replace('/', "~1")
}

fn compact_tool_envelope(value: &mut Value, facts: &mut BoundingFacts) {
    let Some(tools) = value.pointer_mut("/body/tools") else {
        return;
    };
    compact_tool_value(tools, "/body/tools", None, false, facts);
}

fn compact_tool_value(
    value: &mut Value,
    pointer: &str,
    field_name: Option<&str>,
    is_schema_property: bool,
    facts: &mut BoundingFacts,
) {
    if !is_schema_property && matches!(field_name, Some("example" | "examples" | "default")) {
        if value != OMITTED_TOOL_VALUE {
            *value = Value::String(OMITTED_TOOL_VALUE.to_string());
            facts.tool_fields_compacted = facts.tool_fields_compacted.saturating_add(1);
            facts.record(pointer);
        }
        return;
    }
    if field_name == Some("description")
        && let Some(text) = value.as_str()
        && let Some(truncated) = truncated_text(text, TOOL_DESCRIPTION_BYTES)
    {
        *value = Value::String(truncated);
        facts.tool_fields_compacted = facts.tool_fields_compacted.saturating_add(1);
        facts.record(pointer);
        return;
    }
    match value {
        Value::Array(values) => {
            for (index, value) in values.iter_mut().enumerate() {
                compact_tool_value(value, &format!("{pointer}/{index}"), None, false, facts);
            }
        }
        Value::Object(values) => {
            let children_are_schema_entries =
                !is_schema_property && field_name.is_some_and(is_named_schema_map);
            for (key, value) in values {
                compact_tool_value(
                    value,
                    &format!("{pointer}/{}", escape_pointer_segment(key)),
                    Some(key),
                    children_are_schema_entries,
                    facts,
                );
            }
        }
        _ => {}
    }
}

fn is_named_schema_map(field_name: &str) -> bool {
    matches!(
        field_name,
        "$defs" | "definitions" | "dependentSchemas" | "patternProperties" | "properties"
    )
}

fn truncation_metadata_overhead(
    original_size: usize,
    max_bytes: usize,
    items: &[ContentItem],
    facts: &BoundingFacts,
) -> usize {
    let leaves = items.iter().flat_map(|item| item.leaves.iter());
    let paths = facts
        .affected_paths
        .iter()
        .cloned()
        .chain(
            leaves
                .map(|leaf| leaf.pointer.clone())
                .take(MAX_AFFECTED_PATHS.saturating_sub(facts.affected_paths.len())),
        )
        .collect::<Vec<_>>();
    let field_count = facts
        .truncated_field_count
        .saturating_add(items.iter().map(|item| item.leaves.len()).sum());
    let metadata = truncation_metadata(
        original_size,
        max_bytes,
        original_size,
        field_count,
        facts.tool_fields_compacted,
        facts.known_large_fields_truncated,
        &paths,
    );
    let wrapper = json!({"truncation": metadata});
    serialized_size(&wrapper)
        .unwrap_or(usize::MAX)
        .saturating_sub(1)
}

fn insert_truncation_metadata(
    value: &mut Value,
    original_size: usize,
    max_bytes: usize,
    facts: &BoundingFacts,
) {
    let Some(object) = value.as_object_mut() else {
        return;
    };
    let initial = truncation_metadata(
        original_size,
        max_bytes,
        original_size,
        facts.truncated_field_count,
        facts.tool_fields_compacted,
        facts.known_large_fields_truncated,
        &facts.affected_paths,
    );
    object.insert("truncation".to_string(), initial);
    for _ in 0..3 {
        let stored_size = serialized_size(value).unwrap_or(max_bytes);
        let omitted_bytes = original_size.saturating_sub(stored_size);
        let Some(metadata) = value.get_mut("truncation").and_then(Value::as_object_mut) else {
            break;
        };
        metadata.insert("stored_size_bytes".to_string(), json!(stored_size));
        metadata.insert("omitted_bytes".to_string(), json!(omitted_bytes));
    }
}

fn try_attach_truncation_metadata(
    value: &mut Value,
    original_size: usize,
    max_bytes: usize,
    facts: &BoundingFacts,
) -> bool {
    insert_truncation_metadata(value, original_size, max_bytes, facts);
    if serialized_size(value).is_ok_and(|size| size <= max_bytes) {
        return true;
    }
    if let Some(object) = value.as_object_mut() {
        object.remove("truncation");
    }
    false
}

fn truncation_metadata(
    original_size: usize,
    stored_size: usize,
    omitted_bytes: usize,
    truncated_field_count: usize,
    tool_fields_compacted: usize,
    known_large_fields_truncated: usize,
    affected_paths: &[String],
) -> Value {
    json!({
        "strategy_version": STRATEGY_VERSION,
        "original_size_bytes": original_size,
        "stored_size_bytes": stored_size,
        "truncated_field_count": truncated_field_count,
        "omitted_bytes": omitted_bytes,
        "affected_path_count": affected_paths.len(),
        "affected_paths": affected_paths,
        "tool_fields_compacted": tool_fields_compacted,
        "known_large_fields_truncated": known_large_fields_truncated,
    })
}

fn hard_fallback(value: Value, original_size: usize, max_bytes: usize) -> (Value, bool) {
    let mut writer = PrefixCountingWriter::new(max_bytes);
    if serde_json::to_writer(&mut writer, &value).is_err() {
        writer = PrefixCountingWriter::default();
    }
    let mut preview_bytes = max_bytes.min(writer.bytes);
    loop {
        let marker = json!({
            "truncated": true,
            "size_bytes": original_size,
            "preview": String::from_utf8_lossy(&writer.prefix[..preview_bytes]),
        });
        if serialized_size(&marker).is_ok_and(|size| size <= max_bytes) {
            return (marker, true);
        }
        if preview_bytes == 0 {
            return smallest_marker(max_bytes);
        }
        preview_bytes /= 2;
    }
}

fn serialization_failure_marker(max_bytes: usize) -> (Value, bool) {
    let marker = json!({"truncated": true, "error": "payload_serialization_failed"});
    if serialized_size(&marker).is_ok_and(|size| size <= max_bytes) {
        (marker, true)
    } else {
        smallest_marker(max_bytes)
    }
}

fn smallest_marker(max_bytes: usize) -> (Value, bool) {
    for marker in [json!({"truncated": true}), Value::Null, json!(0)] {
        if serialized_size(&marker).is_ok_and(|size| size <= max_bytes) {
            return (marker, true);
        }
    }
    (json!(0), true)
}

#[cfg(test)]
mod tests;
