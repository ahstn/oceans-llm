use super::*;

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct BudgetsConfig {
    #[serde(default)]
    pub users: UserBudgetDefaultsConfig,
}

#[derive(Debug, Clone, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct UserBudgetDefaultsConfig {
    #[serde(default)]
    pub default: Option<BudgetConfig>,
    #[serde(default)]
    pub model_defaults: Vec<UserModelBudgetDefaultConfig>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct UserModelBudgetDefaultConfig {
    pub model: String,
    pub budget: BudgetConfig,
}

#[derive(Debug, Clone, Deserialize)]
pub struct BudgetConfig {
    pub cadence: BudgetCadence,
    pub amount_usd: String,
    #[serde(default = "default_enabled")]
    pub hard_limit: bool,
    #[serde(default = "default_budget_timezone")]
    pub timezone: String,
}

impl BudgetConfig {
    pub(super) fn validate(&self, label: &str) -> anyhow::Result<()> {
        if self.timezone.trim().is_empty() {
            bail!("{label} timezone cannot be empty");
        }
        let amount = Money4::from_decimal_str(&self.amount_usd)
            .map_err(|error| anyhow::anyhow!("{label} amount_usd is invalid: {error}"))?;
        if amount.is_negative() {
            bail!("{label} amount_usd cannot be negative");
        }
        Ok(())
    }

    pub(super) fn seed_budget(&self) -> anyhow::Result<SeedBudget> {
        let amount_usd = Money4::from_decimal_str(&self.amount_usd).map_err(|error| {
            anyhow::anyhow!("invalid amount_usd `{}`: {error}", self.amount_usd)
        })?;
        Ok(SeedBudget {
            cadence: self.cadence,
            amount_usd,
            hard_limit: self.hard_limit,
            timezone: self.timezone.trim().to_string(),
        })
    }
}

pub(super) fn validate_user_defaults(
    budgets: &BudgetsConfig,
    model_by_id: &BTreeMap<&str, &ModelConfig>,
) -> anyhow::Result<()> {
    if let Some(default_budget) = &budgets.users.default {
        default_budget.validate("budgets.users.default")?;
    }

    let mut model_defaults = std::collections::BTreeSet::new();
    for model_default in &budgets.users.model_defaults {
        let model_key = normalize_config_model_key(&model_default.model)
            .context("budgets.users.model_defaults model")?;
        if !model_by_id.contains_key(model_key.as_str()) {
            bail!("budgets.users.model_defaults references unknown model `{model_key}`");
        }
        if !model_defaults.insert(model_key.clone()) {
            bail!("duplicate budgets.users.model_defaults model `{model_key}`");
        }
        model_default.budget.validate(&format!(
            "budgets.users.model_defaults `{model_key}` budget"
        ))?;
    }

    Ok(())
}
