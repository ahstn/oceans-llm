use std::collections::BTreeMap;

use gateway_core::{FocusExportAggregateRecord, FocusExportDiagnosticsRecord, Money4, RequestTag};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::http::admin_contract::format_timestamp;

const PROVIDER_NAME: &str = "Oceans LLM Gateway";
const BILLING_ACCOUNT_ID: &str = "oceans-llm";
const BILLING_ACCOUNT_NAME: &str = "Oceans LLM Gateway";
const SERVICE_NAME: &str = "LLM Gateway";

const FOCUS_HEADERS: &[&str] = &[
    "ProviderName",
    "PublisherName",
    "BillingAccountId",
    "BillingAccountName",
    "SubAccountId",
    "SubAccountName",
    "ChargePeriodStart",
    "ChargePeriodEnd",
    "ChargeCategory",
    "ChargeClass",
    "BillingCurrency",
    "BilledCost",
    "EffectiveCost",
    "ListCost",
    "ContractedCost",
    "ServiceCategory",
    "ServiceName",
    "SkuId",
    "SkuPriceId",
    "ConsumedQuantity",
    "ConsumedUnit",
    "PricingQuantity",
    "PricingUnit",
    "RegionName",
    "ResourceId",
    "ResourceName",
    "Tags",
];

const CUSTOM_HEADERS: &[&str] = &[
    "x_owner_kind",
    "x_owner_id",
    "x_owner_name",
    "x_upstream_provider",
    "x_upstream_model",
    "x_model_id",
    "x_prompt_tokens",
    "x_completion_tokens",
    "x_total_tokens",
    "x_request_count",
    "x_pricing_status",
];

pub(crate) struct FocusCsvExport {
    pub(crate) filename: String,
    pub(crate) body: String,
    pub(crate) diagnostics: FocusExportDiagnosticsRecord,
}

pub(crate) fn build_focus_csv_export(
    rows: &[FocusExportAggregateRecord],
    diagnostics: FocusExportDiagnosticsRecord,
    window_start: OffsetDateTime,
    window_end: OffsetDateTime,
) -> FocusCsvExport {
    let mut body = String::new();
    write_record(
        &mut body,
        FOCUS_HEADERS.iter().chain(CUSTOM_HEADERS.iter()).copied(),
    );
    for row in rows {
        write_record(&mut body, focus_row_values(row));
    }

    FocusCsvExport {
        filename: focus_filename(window_start, window_end),
        body,
        diagnostics,
    }
}

fn focus_row_values(row: &FocusExportAggregateRecord) -> Vec<String> {
    let day_end = row.day_start + Duration::days(1);
    let cost = format_money(row.computed_cost_usd);
    let resource_id = resource_id(row);
    let resource_name = format!("{} / {}", row.owner_name, row.model_key);
    let model_id = row.model_id.map(|id| id.to_string()).unwrap_or_default();
    let pricing_row_id = row
        .pricing_row_id
        .map(|id| id.to_string())
        .unwrap_or_default();

    vec![
        PROVIDER_NAME.to_string(),
        PROVIDER_NAME.to_string(),
        BILLING_ACCOUNT_ID.to_string(),
        BILLING_ACCOUNT_NAME.to_string(),
        row.owner_id.to_string(),
        row.owner_name.clone(),
        format_timestamp(row.day_start),
        format_timestamp(day_end),
        "Usage".to_string(),
        String::new(),
        "USD".to_string(),
        cost.clone(),
        cost.clone(),
        cost.clone(),
        cost,
        "AI and Machine Learning".to_string(),
        SERVICE_NAME.to_string(),
        row.model_key.clone(),
        pricing_row_id,
        row.total_tokens.to_string(),
        "tokens".to_string(),
        format_per_million_quantity(row.total_tokens),
        "1M tokens".to_string(),
        String::new(),
        resource_id,
        resource_name,
        focus_tags(&row.owner_tags),
        row.owner_kind.as_str().to_string(),
        row.owner_id.to_string(),
        row.owner_name.clone(),
        row.provider_key.clone(),
        row.upstream_model.clone(),
        model_id,
        row.prompt_tokens.to_string(),
        row.completion_tokens.to_string(),
        row.total_tokens.to_string(),
        row.request_count.to_string(),
        row.pricing_status.as_str().to_string(),
    ]
}

fn focus_tags(tags: &[RequestTag]) -> String {
    if tags.is_empty() {
        return String::new();
    }
    let tag_map = tags
        .iter()
        .map(|tag| (tag.key.clone(), tag.value.clone()))
        .collect::<BTreeMap<_, _>>();
    serde_json::to_string(&tag_map).unwrap_or_default()
}

fn write_record<S>(body: &mut String, values: impl IntoIterator<Item = S>)
where
    S: AsRef<str>,
{
    let mut first = true;
    for value in values {
        if !first {
            body.push(',');
        }
        first = false;
        write_csv_field(body, value.as_ref());
    }
    body.push('\n');
}

fn write_csv_field(body: &mut String, value: &str) {
    let sanitized;
    let value = if should_neutralize_spreadsheet_formula(value) {
        sanitized = format!("'{value}");
        sanitized.as_str()
    } else {
        value
    };

    let must_quote = value.contains(',')
        || value.contains('"')
        || value.contains('\n')
        || value.contains('\r')
        || value.starts_with(' ')
        || value.ends_with(' ');
    if !must_quote {
        body.push_str(value);
        return;
    }
    body.push('"');
    for ch in value.chars() {
        if ch == '"' {
            body.push('"');
        }
        body.push(ch);
    }
    body.push('"');
}

fn should_neutralize_spreadsheet_formula(value: &str) -> bool {
    let trimmed = value.trim_start_matches([' ', '\t', '\r', '\n']);
    match trimmed.as_bytes().first().copied() {
        Some(b'=' | b'+' | b'@') => true,
        Some(b'-') => !looks_like_negative_number(trimmed),
        _ => false,
    }
}

fn looks_like_negative_number(value: &str) -> bool {
    let Some(rest) = value.strip_prefix('-') else {
        return false;
    };
    let Some((whole, fractional)) = rest.split_once('.') else {
        return rest.chars().all(|ch| ch.is_ascii_digit());
    };
    !whole.is_empty()
        && !fractional.is_empty()
        && whole.chars().all(|ch| ch.is_ascii_digit())
        && fractional.chars().all(|ch| ch.is_ascii_digit())
}

fn resource_id(row: &FocusExportAggregateRecord) -> String {
    let input = format!(
        "focus:{}:{}:{}:{}:{}:{}:{}:{}:{}",
        row.day_start.unix_timestamp(),
        row.owner_kind.as_str(),
        row.owner_id,
        row.provider_key,
        row.upstream_model,
        row.model_key,
        row.model_id.map(|id| id.to_string()).unwrap_or_default(),
        row.pricing_row_id
            .map(|id| id.to_string())
            .unwrap_or_default(),
        row.pricing_status.as_str()
    );
    Uuid::new_v5(&Uuid::NAMESPACE_URL, input.as_bytes()).to_string()
}

fn focus_filename(window_start: OffsetDateTime, window_end: OffsetDateTime) -> String {
    let start = window_start.date();
    let end_inclusive = (window_end - Duration::days(1)).date();
    if start == end_inclusive {
        format!("oceans-focus-{start}.csv")
    } else {
        format!("oceans-focus-{start}-to-{end_inclusive}.csv")
    }
}

fn format_money(value: Money4) -> String {
    let scaled = value.as_scaled_i64();
    let sign = if scaled < 0 { "-" } else { "" };
    let abs = scaled.abs();
    format!("{sign}{}.{:04}", abs / Money4::SCALE, abs % Money4::SCALE)
}

fn format_per_million_quantity(tokens: i64) -> String {
    let sign = if tokens < 0 { "-" } else { "" };
    let abs = tokens.abs();
    format!("{sign}{}.{:06}", abs / 1_000_000, abs % 1_000_000)
}

#[cfg(test)]
mod tests {
    use super::*;
    use gateway_core::{ApiKeyOwnerKind, UsagePricingStatus};
    use time::macros::datetime;

    #[test]
    fn renders_headers_before_custom_columns() {
        let export = build_focus_csv_export(
            &[],
            FocusExportDiagnosticsRecord::default(),
            datetime!(2026-05-19 0:00 UTC),
            datetime!(2026-05-20 0:00 UTC),
        );

        let header = export.body.lines().next().expect("header");
        assert!(header.starts_with("ProviderName,PublisherName"));
        assert!(header.contains(",ResourceName,Tags,x_owner_kind,"));
    }

    #[test]
    fn escapes_csv_fields_and_formats_money() {
        let row = sample_row("gpt-test");

        let export = build_focus_csv_export(
            &[row],
            FocusExportDiagnosticsRecord::default(),
            datetime!(2026-05-19 0:00 UTC),
            datetime!(2026-05-20 0:00 UTC),
        );

        assert!(export.body.contains("1.2345"));
        assert!(export.body.contains("\"A, \"\"quoted\"\" user\""));
        assert!(export.body.contains(",0.000003,"));
    }

    #[test]
    fn renders_owner_tags_as_focus_tags_json() {
        let mut row = sample_row("gpt-test");
        row.owner_tags = vec![
            RequestTag {
                key: "team".to_string(),
                value: "platform".to_string(),
            },
            RequestTag {
                key: "env".to_string(),
                value: "prod".to_string(),
            },
        ];

        let export = build_focus_csv_export(
            &[row],
            FocusExportDiagnosticsRecord::default(),
            datetime!(2026-05-19 0:00 UTC),
            datetime!(2026-05-20 0:00 UTC),
        );

        assert!(
            export
                .body
                .contains(r#""{""env"":""prod"",""team"":""platform""}""#)
        );
    }

    #[test]
    fn resource_id_changes_for_distinct_grouping_dimensions() {
        let first = sample_row("gpt-test");
        let mut second = sample_row("gpt-test-alias");
        second.pricing_row_id =
            Some(Uuid::parse_str("00000000-0000-0000-0000-000000000222").unwrap());

        assert_ne!(resource_id(&first), resource_id(&second));
    }

    #[test]
    fn neutralizes_formula_like_csv_fields() {
        let mut row = sample_row("gpt-test");
        row.owner_name = "=IMPORTXML(\"https://example.test\")".to_string();
        row.upstream_model = "-not-a-number".to_string();

        let export = build_focus_csv_export(
            &[row],
            FocusExportDiagnosticsRecord::default(),
            datetime!(2026-05-19 0:00 UTC),
            datetime!(2026-05-20 0:00 UTC),
        );

        assert!(export.body.contains("'=IMPORTXML"));
        assert!(export.body.contains("'-not-a-number"));
        assert!(should_neutralize_spreadsheet_formula("\t=1+1"));
        assert!(should_neutralize_spreadsheet_formula("\r+SUM(A1:A2)"));
        assert!(should_neutralize_spreadsheet_formula(" =IMPORTXML"));
        assert!(!should_neutralize_spreadsheet_formula("-1.2345"));
    }

    fn sample_row(model_key: &str) -> FocusExportAggregateRecord {
        FocusExportAggregateRecord {
            day_start: datetime!(2026-05-19 0:00 UTC),
            owner_kind: ApiKeyOwnerKind::User,
            owner_id: Uuid::parse_str("00000000-0000-0000-0000-000000000111").unwrap(),
            owner_name: "A, \"quoted\" user".to_string(),
            owner_tags: Vec::new(),
            model_id: None,
            model_key: model_key.to_string(),
            provider_key: "openai".to_string(),
            upstream_model: "gpt-test".to_string(),
            pricing_status: UsagePricingStatus::Priced,
            pricing_row_id: None,
            prompt_tokens: 1,
            completion_tokens: 2,
            total_tokens: 3,
            request_count: 1,
            computed_cost_usd: Money4::from_scaled(12_345),
        }
    }
}
