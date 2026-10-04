//! Bound decoded YAML before aliases can expand into an unbounded JSON tree.

use std::fmt;

use serde::de::{self, DeserializeSeed, MapAccess, SeqAccess, Visitor};
use serde_json::{Map, Number, Value};

const MAX_SCALAR_BYTES: usize = 128 * 1024;
const MAX_NODES: usize = 4096;
const MAX_DEPTH: usize = 32;

pub(crate) fn parse_frontmatter(yaml: &str) -> Result<Value, String> {
    ValueSeed {
        budget: &mut Budget::default(),
        depth: 0,
    }
    .deserialize(serde_yaml::Deserializer::from_str(yaml))
    .map_err(|error| error.to_string())
}

#[derive(Default)]
struct Budget {
    scalar_bytes: usize,
    nodes: usize,
}

impl Budget {
    fn enter<E: de::Error>(&mut self, depth: usize) -> Result<(), E> {
        if depth > MAX_DEPTH {
            return Err(E::custom("frontmatter exceeds 32 levels of nesting"));
        }
        if self.nodes >= MAX_NODES {
            return Err(E::custom("frontmatter exceeds 4096 decoded nodes"));
        }
        self.nodes += 1;
        Ok(())
    }

    fn scalar<E: de::Error>(&mut self, bytes: usize) -> Result<(), E> {
        if bytes > MAX_SCALAR_BYTES - self.scalar_bytes {
            return Err(E::custom(
                "frontmatter exceeds 128 KiB of decoded scalar data",
            ));
        }
        self.scalar_bytes += bytes;
        Ok(())
    }

    fn text<E: de::Error>(&mut self, value: &str) -> Result<(), E> {
        // PostgreSQL text and jsonb cannot represent U+0000, including YAML escapes.
        if value.contains('\0') {
            return Err(E::custom("frontmatter must not contain NUL characters"));
        }
        self.scalar(value.len())
    }
}

struct ValueSeed<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for ValueSeed<'_> {
    type Value = Value;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<Value, D::Error> {
        self.budget.enter(self.depth)?;
        deserializer.deserialize_any(ValueVisitor {
            budget: self.budget,
            depth: self.depth,
        })
    }
}

struct ValueVisitor<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl ValueVisitor<'_> {
    fn number<E: de::Error>(self, value: Number) -> Result<Value, E> {
        self.budget.scalar(value.to_string().len())?;
        Ok(Value::Number(value))
    }
}

impl<'de> Visitor<'de> for ValueVisitor<'_> {
    type Value = Value;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("JSON-compatible YAML without tags")
    }

    fn visit_unit<E: de::Error>(self) -> Result<Value, E> {
        self.budget.scalar(4)?;
        Ok(Value::Null)
    }

    fn visit_bool<E: de::Error>(self, value: bool) -> Result<Value, E> {
        self.budget.scalar(if value { 4 } else { 5 })?;
        Ok(Value::Bool(value))
    }

    fn visit_i64<E: de::Error>(self, value: i64) -> Result<Value, E> {
        self.number(value.into())
    }

    fn visit_u64<E: de::Error>(self, value: u64) -> Result<Value, E> {
        self.number(value.into())
    }

    fn visit_f64<E: de::Error>(self, value: f64) -> Result<Value, E> {
        self.number(
            Number::from_f64(value)
                .ok_or_else(|| E::custom("frontmatter numbers must be finite"))?,
        )
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<Value, E> {
        self.budget.text(value)?;
        Ok(Value::String(value.to_owned()))
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<Value, E> {
        self.budget.text(&value)?;
        Ok(Value::String(value))
    }

    fn visit_seq<A: SeqAccess<'de>>(self, mut sequence: A) -> Result<Value, A::Error> {
        // Do not reserve from an input-controlled size hint.
        let mut values = Vec::new();
        while let Some(value) = sequence.next_element_seed(ValueSeed {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            values.push(value);
        }
        Ok(Value::Array(values))
    }

    fn visit_map<A: MapAccess<'de>>(self, mut mapping: A) -> Result<Value, A::Error> {
        let mut values = Map::new();
        while let Some(key) = mapping.next_key_seed(KeySeed {
            budget: self.budget,
            depth: self.depth + 1,
        })? {
            if values.contains_key(&key) {
                return Err(de::Error::custom(
                    "frontmatter contains a duplicate field name",
                ));
            }
            let value = mapping.next_value_seed(ValueSeed {
                budget: self.budget,
                depth: self.depth + 1,
            })?;
            values.insert(key, value);
        }
        Ok(Value::Object(values))
    }
}

struct KeySeed<'a> {
    budget: &'a mut Budget,
    depth: usize,
}

impl<'de> DeserializeSeed<'de> for KeySeed<'_> {
    type Value = String;

    fn deserialize<D: de::Deserializer<'de>>(self, deserializer: D) -> Result<String, D::Error> {
        self.budget.enter(self.depth)?;
        // deserialize_any keeps YAML numeric keys from being coerced into strings.
        deserializer.deserialize_any(KeyVisitor(self.budget))
    }
}

struct KeyVisitor<'a>(&'a mut Budget);

impl Visitor<'_> for KeyVisitor<'_> {
    type Value = String;

    fn expecting(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str("a string field name")
    }

    fn visit_str<E: de::Error>(self, value: &str) -> Result<String, E> {
        self.0.text(value)?;
        Ok(value.to_owned())
    }

    fn visit_string<E: de::Error>(self, value: String) -> Result<String, E> {
        self.0.text(&value)?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::parse_frontmatter;
    use serde_json::json;

    #[test]
    fn accepts_multiline_frontmatter_and_json_extensions() {
        let value = parse_frontmatter(
            "name: code-review\ndescription: >\n  Review code\n  before merge.\nmetadata:\n  owner: platform\nx-options:\n  enabled: true\n  weights: [1, 0.5, null]\n",
        ).unwrap();
        assert_eq!(value["description"], "Review code before merge.\n");
        assert_eq!(value["metadata"], json!({"owner": "platform"}));
        assert_eq!(
            value["x-options"],
            json!({"enabled": true, "weights": [1, 0.5, null]})
        );
    }

    #[test]
    fn bounds_alias_scalar_expansion_before_copying_each_value() {
        let mut yaml = format!("shared: &shared {}\nrepeated:\n", "x".repeat(32 * 1024));
        for _ in 0..8 {
            yaml.push_str("  - *shared\n");
        }
        assert!(yaml.len() < 64 * 1024);
        assert!(parse_frontmatter(&yaml).unwrap_err().contains("128 KiB"));
    }

    #[test]
    fn bounds_alias_node_expansion_and_nesting() {
        let yaml = format!(
            "base: &base [0, 0, 0, 0, 0, 0, 0, 0]\nnext: &next [{}]\nlast: [{}]\n",
            ["*base"; 8].join(", "),
            ["*next"; 64].join(", "),
        );
        assert!(parse_frontmatter(&yaml).unwrap_err().contains("4096"));
        let nested = format!("value: {}0{}", "[".repeat(33), "]".repeat(33));
        assert!(parse_frontmatter(&nested).unwrap_err().contains("nesting"));
    }

    #[test]
    fn rejects_duplicate_keys_tags_and_non_json_values() {
        for yaml in [
            "name: first\nname: second\n",
            "options: {name: first, name: second}",
            "name: !custom code-review",
            "name: {.nan: value}",
            "name: .inf",
            "options: {1: value}",
            "name: first\n---\nname: second",
            "description: \"Review\\0code\"",
            "metadata: {author: \"User\\u0000name\"}",
            "x-options: {\"bad\\0key\": true}",
        ] {
            assert!(parse_frontmatter(yaml).is_err(), "accepted {yaml}");
        }
    }
}
