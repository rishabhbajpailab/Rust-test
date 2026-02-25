//! Plant-type metric threshold evaluation.

use serde::{Deserialize, Serialize};

// ------------------------------------------------------------------ //
//  Types                                                              //
// ------------------------------------------------------------------ //

/// Severity level for an individual metric or an entire plant.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub enum Severity {
    Normal,
    Warn,
    Critical,
}

impl Severity {
    pub fn as_str(self) -> &'static str {
        match self {
            Severity::Normal   => "NORMAL",
            Severity::Warn     => "WARN",
            Severity::Critical => "CRITICAL",
        }
    }

    pub fn from_str(s: &str) -> Self {
        match s {
            "WARN"     => Severity::Warn,
            "CRITICAL" => Severity::Critical,
            _          => Severity::Normal,
        }
    }
}

impl std::fmt::Display for Severity {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.as_str())
    }
}

/// Threshold definition for a single metric.
#[derive(Debug, Clone)]
pub struct MetricThreshold {
    pub metric:   String,
    pub warn_min: Option<f64>,
    pub warn_max: Option<f64>,
    pub crit_min: Option<f64>,
    pub crit_max: Option<f64>,
}

// ------------------------------------------------------------------ //
//  Evaluation                                                         //
// ------------------------------------------------------------------ //

/// Evaluate a single reading against its threshold.
pub fn evaluate_metric(value: f64, threshold: &MetricThreshold) -> Severity {
    if let Some(min) = threshold.crit_min {
        if value < min {
            return Severity::Critical;
        }
    }
    if let Some(max) = threshold.crit_max {
        if value > max {
            return Severity::Critical;
        }
    }
    if let Some(min) = threshold.warn_min {
        if value < min {
            return Severity::Warn;
        }
    }
    if let Some(max) = threshold.warn_max {
        if value > max {
            return Severity::Warn;
        }
    }
    Severity::Normal
}

/// Compute the overall plant severity from per-metric severities.
pub fn aggregate_severity(severities: impl IntoIterator<Item = Severity>) -> Severity {
    let mut overall = Severity::Normal;
    for s in severities {
        if s > overall {
            overall = s;
        }
    }
    overall
}

// ------------------------------------------------------------------ //
//  Tests                                                              //
// ------------------------------------------------------------------ //

#[cfg(test)]
mod tests {
    use super::*;

    fn thresh(
        warn_min: Option<f64>,
        warn_max: Option<f64>,
        crit_min: Option<f64>,
        crit_max: Option<f64>,
    ) -> MetricThreshold {
        MetricThreshold { metric: "test".into(), warn_min, warn_max, crit_min, crit_max }
    }

    #[test]
    fn normal_when_within_warn_band() {
        let t = thresh(Some(20.0), Some(80.0), Some(10.0), Some(90.0));
        assert_eq!(evaluate_metric(50.0, &t), Severity::Normal);
    }

    #[test]
    fn warn_when_below_warn_min() {
        let t = thresh(Some(20.0), Some(80.0), Some(10.0), Some(90.0));
        assert_eq!(evaluate_metric(15.0, &t), Severity::Warn);
    }

    #[test]
    fn warn_when_above_warn_max() {
        let t = thresh(Some(20.0), Some(80.0), Some(10.0), Some(90.0));
        assert_eq!(evaluate_metric(85.0, &t), Severity::Warn);
    }

    #[test]
    fn critical_when_below_crit_min() {
        let t = thresh(Some(20.0), Some(80.0), Some(10.0), Some(90.0));
        assert_eq!(evaluate_metric(5.0, &t), Severity::Critical);
    }

    #[test]
    fn critical_when_above_crit_max() {
        let t = thresh(Some(20.0), Some(80.0), Some(10.0), Some(90.0));
        assert_eq!(evaluate_metric(95.0, &t), Severity::Critical);
    }

    #[test]
    fn no_crit_bounds_never_critical() {
        let t = thresh(Some(20.0), Some(80.0), None, None);
        assert_eq!(evaluate_metric(5.0, &t), Severity::Warn);
    }

    #[test]
    fn no_bounds_always_normal() {
        let t = thresh(None, None, None, None);
        assert_eq!(evaluate_metric(0.0, &t), Severity::Normal);
        assert_eq!(evaluate_metric(100.0, &t), Severity::Normal);
    }

    #[test]
    fn aggregate_any_critical_wins() {
        let result = aggregate_severity([Severity::Normal, Severity::Critical, Severity::Warn]);
        assert_eq!(result, Severity::Critical);
    }

    #[test]
    fn aggregate_any_warn_without_critical() {
        let result = aggregate_severity([Severity::Normal, Severity::Warn, Severity::Normal]);
        assert_eq!(result, Severity::Warn);
    }

    #[test]
    fn aggregate_all_normal() {
        let result = aggregate_severity([Severity::Normal, Severity::Normal]);
        assert_eq!(result, Severity::Normal);
    }

    #[test]
    fn aggregate_empty_is_normal() {
        let result = aggregate_severity(std::iter::empty());
        assert_eq!(result, Severity::Normal);
    }

    #[test]
    fn no_transition_emit_same_severity() {
        let prev = Severity::Warn;
        let next = Severity::Warn;
        assert_eq!(prev, next, "warn->warn should not emit");

        let prev = Severity::Critical;
        let next = Severity::Critical;
        assert_eq!(prev, next, "critical->critical should not emit");
    }

    #[test]
    fn transition_emitted_on_change() {
        let prev = Severity::Normal;
        let next = Severity::Warn;
        assert_ne!(prev, next, "normal->warn should emit");

        let prev = Severity::Warn;
        let next = Severity::Critical;
        assert_ne!(prev, next, "warn->critical should emit");

        let prev = Severity::Critical;
        let next = Severity::Normal;
        assert_ne!(prev, next, "critical->normal should emit");
    }
}
