use warp_graphql::billing::{OveragesPricing, PlanPricing, PricingInfo, StripeSubscriptionPlan};
use warpui::{Entity, SingletonEntity};

/// A global model for maintaining pricing information from the server.
#[derive(Debug)]
pub struct PricingInfoModel {
    /// The latest-known pricing information from the server.
    pricing_info: Option<PricingInfo>,
}

impl PricingInfoModel {
    pub fn new() -> Self {
        Self { pricing_info: None }
    }

    /// Returns the current overage pricing information.
    #[allow(dead_code)]
    fn overage_pricing(&self) -> Option<&OveragesPricing> {
        self.pricing_info.as_ref().map(|info| &info.overages)
    }

    /// Returns the pricing for a specific plan.
    #[allow(dead_code)]
    pub fn plan_pricing(&self, plan: &StripeSubscriptionPlan) -> Option<&PlanPricing> {
        self.pricing_info
            .as_ref()?
            .plans
            .iter()
            .find(|p| &p.plan == plan)
    }

    /// Returns the overage cost in dollars (converted from cents).
    #[allow(dead_code)]
    pub fn overage_cost_dollars(&self) -> Option<f64> {
        self.overage_pricing()
            .map(|overages| overages.price_per_request_usd_cents as f64 / 100.0)
    }

    /// Returns the monthly cost for a plan in dollars (converted from cents).
    #[allow(dead_code)]
    pub fn monthly_plan_cost_dollars(&self, plan: &StripeSubscriptionPlan) -> Option<f64> {
        self.plan_pricing(plan)
            .map(|pricing| pricing.monthly_plan_price_per_month_usd_cents as f64 / 100.0)
    }

    pub fn promotion_message(&self) -> Option<&str> {
        self.pricing_info.as_ref()?.promotion_message.as_deref()
    }
}

impl Default for PricingInfoModel {
    fn default() -> Self {
        Self::new()
    }
}

impl Entity for PricingInfoModel {
    type Event = ();
}

impl SingletonEntity for PricingInfoModel {}

#[cfg(test)]
#[path = "pricing_tests.rs"]
mod tests;
