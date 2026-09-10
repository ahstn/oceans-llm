"""Build a reviewable LiteLLM supplement from local source files. No network access."""
import argparse
from datetime import datetime, timezone
from decimal import Decimal
import hashlib
import json
from pathlib import Path


def per_million(value):
    amount = Decimal(str(value))
    if not amount.is_finite() or amount < 0:
        raise ValueError("Token prices must be finite and nonnegative")
    return format(amount * Decimal(1_000_000), "f")


def project(models_dev, litellm):
    # Start with one unambiguous namespace. Do not infer cloud regions, tiers,
    # aliases, or cross-provider identities from similar model names.
    records = {}
    for model_id in models_dev.get("openai", {}).get("models", {}):
        row = litellm.get(model_id)
        if not row or row.get("litellm_provider") != "openai" or row.get("supported_regions"):
            continue
        metadata = {}
        for target, source in [("reasoning", "supports_reasoning"),
                               ("tool_call", "supports_function_calling"),
                               ("structured_output", "supports_response_schema")]:
            if isinstance(row.get(source), bool):
                metadata[target] = row[source]
        pricing = {}
        for target, source in [("input", "input_cost_per_token"),
                               ("output", "output_cost_per_token"),
                               ("cache_read", "cache_read_input_token_cost"),
                               ("cache_write", "cache_creation_input_token_cost")]:
            if row.get(source) is not None:
                pricing[target] = per_million(row[source])
        limits = {}
        for target, source in [("input", "max_input_tokens"), ("output", "max_output_tokens")]:
            value = row.get(source)
            if isinstance(value, int) and not isinstance(value, bool) and value > 0:
                limits[target] = value
        records[model_id] = {"metadata": metadata, "pricing": pricing,
                             "limits": limits, "deprecated_date": row.get("deprecation_date")}
    return records


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("models_dev", type=Path)
    parser.add_argument("litellm", type=Path)
    parser.add_argument("output", type=Path)
    args = parser.parse_args()
    primary = args.models_dev.read_bytes()
    secondary = args.litellm.read_bytes()
    result = {
        "source": "litellm", "provider_id": "openai",
        "generated_at": datetime.now(timezone.utc).isoformat(),
        "models_dev_sha256": hashlib.sha256(primary).hexdigest(),
        "litellm_sha256": hashlib.sha256(secondary).hexdigest(),
        "models": project(json.loads(primary), json.loads(secondary, parse_float=Decimal)),
    }
    args.output.write_text(json.dumps(result, indent=2, sort_keys=True) + "\n")


if __name__ == "__main__":
    main()
