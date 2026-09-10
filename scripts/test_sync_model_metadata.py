import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location("sync_metadata", Path(__file__).with_name("sync-model-metadata.py"))
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class ProjectionTests(unittest.TestCase):
    def test_decimal_conversion_preserves_small_values_and_zero(self):
        self.assertEqual(module.per_million("0.000000000123"), "0.000123000000")
        self.assertEqual(module.per_million(0), "0")
        for value in ["NaN", "Infinity", "-1"]:
            with self.assertRaises(ValueError):
                module.per_million(value)

    def test_exact_identity_and_missing_semantics(self):
        primary = {"openai": {"models": {key: {} for key in ["a", "b", "c", "d"]}}}
        secondary = {
            "a": {"litellm_provider": "openai", "supports_reasoning": False, "max_tokens": 999, "input_cost_per_token": 0},
            "b": {"litellm_provider": "azure"},
            "c": {"litellm_provider": "openai", "supported_regions": ["us"]},
            "openai/d": {"litellm_provider": "openai"},
        }
        records = module.project(primary, secondary)
        self.assertEqual(list(records), ["a"])
        self.assertEqual(records["a"]["metadata"], {"reasoning": False})
        self.assertEqual(records["a"]["pricing"], {"input": "0"})
        self.assertEqual(records["a"]["limits"], {})


if __name__ == "__main__":
    unittest.main()
