//! Dynamic text is resolved here, once, for editor previews and every wire format.
use crate::deepseek;
use pixelferry_core::{DeepSeekPreferences, RichRun, RichStyle, RichText, SensorSample};

pub struct ResolvedRun {
    pub text: String,
    pub style: RichStyle,
    pub font_scale: f32,
}

pub fn rounded_money(value: &str, decimals: u8) -> Option<String> {
    if decimals > 8 {
        return None;
    }
    let n = deepseek::units(value).ok()?;
    let divisor = 10u128.pow(8 - decimals as u32);
    let mut rounded = n.unsigned_abs() / divisor;
    // Balance responses can contain more than eight decimals. Inspect the
    // original next digit rather than rounding an already truncated ledger value.
    let fraction = value.split_once('.').map_or("", |(_, fraction)| fraction);
    if fraction
        .as_bytes()
        .get(decimals as usize)
        .is_some_and(|digit| *digit >= b'5')
    {
        rounded += 1;
    }
    let sign = if value.starts_with('-') && rounded != 0 {
        "-"
    } else {
        ""
    };
    if decimals == 0 {
        return Some(format!("{sign}{rounded}"));
    }
    let scale = 10u128.pow(decimals as u32);
    Some(format!(
        "{sign}{}.{:0width$}",
        rounded / scale,
        rounded % scale,
        width = decimals as usize
    ))
}

pub fn resolve(
    rich: &RichText,
    sensors: &[SensorSample],
    account: &deepseek::Snapshot,
    prefs: &DeepSeekPreferences,
    now: i64,
) -> Vec<ResolvedRun> {
    rich.runs
        .iter()
        .flat_map(|run| {
            let text = match run {
                RichRun::Text { text, .. } => text.clone(),
                RichRun::Sensor {
                    sensor_id,
                    decimals,
                    show_unit,
                    ..
                } => sensors
                    .iter()
                    .find(|s| &s.id == sensor_id)
                    .filter(|s| s.status == "ok" && (0..=5000).contains(&(now - s.timestamp)))
                    .and_then(|s| {
                        s.value.filter(|v| v.is_finite()).map(|v| {
                            let suffix = if *show_unit && !s.unit.is_empty() {
                                format!(" {}", s.unit)
                            } else {
                                String::new()
                            };
                            format!("{v:.*}{suffix}", *decimals as usize)
                        })
                    })
                    .unwrap_or_else(|| "--".into()),
                RichRun::Deepseek {
                    metric,
                    decimals,
                    show_currency,
                    ..
                } => {
                    if metric == "period" {
                        deepseek::text("period", account, prefs, now)
                    } else if !account.configured
                        || account.status != "ok"
                        || !(0..=180000).contains(&(now - account.timestamp))
                    {
                        "--".into()
                    } else {
                        let balance = account
                            .balances
                            .iter()
                            .find(|b| b.currency == prefs.currency);
                        let value = balance.and_then(|b| match metric.as_str() {
                            "balance" => Some(b.total.as_str()),
                            "granted" => Some(b.granted.as_str()),
                            "toppedUp" => Some(b.topped_up.as_str()),
                            "today" if deepseek::day(account.timestamp) != deepseek::day(now) => {
                                Some("0")
                            }
                            "today" => b.today_spent.as_deref(),
                            "total" => b.total_spent.as_deref(),
                            _ => None,
                        });
                        value
                            .and_then(|v| rounded_money(v, *decimals))
                            .map(|v| {
                                let symbol = if !show_currency {
                                    ""
                                } else if prefs.currency == "CNY" {
                                    "¥"
                                } else {
                                    "$"
                                };
                                format!("{symbol}{v}")
                            })
                            .unwrap_or_else(|| "--".into())
                    }
                }
            };
            let piece = |text: String, font_scale| ResolvedRun {
                text,
                style: run.style().clone(),
                font_scale,
            };
            let scale = run.affix_scale();
            if scale < 1. {
                match run {
                    RichRun::Sensor {
                        sensor_id,
                        show_unit: true,
                        ..
                    } => {
                        if let Some(sensor) = sensors
                            .iter()
                            .find(|s| &s.id == sensor_id && !s.unit.is_empty())
                        {
                            let suffix = format!(" {}", sensor.unit);
                            if let Some(value) = text.strip_suffix(&suffix) {
                                return vec![piece(value.into(), 1.), piece(suffix, scale)];
                            }
                        }
                    }
                    RichRun::Deepseek {
                        show_currency: true,
                        metric,
                        ..
                    } if metric != "period" => {
                        let symbol = if prefs.currency == "CNY" { "¥" } else { "$" };
                        if let Some(value) = text.strip_prefix(symbol) {
                            return vec![piece(symbol.into(), scale), piece(value.into(), 1.)];
                        }
                    }
                    _ => {}
                }
            }
            vec![piece(text, 1.)]
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn only_visible_affixes_shrink_and_missing_values_stay_intact() {
        let sensors = vec![SensorSample {
            id: "gpu".into(),
            name: "GPU".into(),
            unit: "°C".into(),
            value: Some(65.),
            timestamp: 1000,
            status: "ok".into(),
            source_id: 0,
            gpu_index: 0,
            device: String::new(),
        }];
        let account = deepseek::Snapshot {
            configured: true,
            status: "ok".into(),
            timestamp: 1000,
            balances: vec![deepseek::Balance {
                currency: "CNY".into(),
                total: "12.5".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let rich = RichText {
            version: 1,
            runs: vec![
                RichRun::Deepseek {
                    metric: "balance".into(),
                    decimals: 2,
                    show_currency: true,
                    affix_scale: Some(0.7),
                    style: RichStyle {
                        font_size: Some(40.),
                        ..Default::default()
                    },
                },
                RichRun::Sensor {
                    sensor_id: "gpu".into(),
                    label: "GPU".into(),
                    decimals: 1,
                    show_unit: true,
                    affix_scale: Some(0.5),
                    style: RichStyle::default(),
                },
            ],
        };
        rich.validate().unwrap();
        let resolved = resolve(
            &rich,
            &sensors,
            &account,
            &DeepSeekPreferences::default(),
            1000,
        );
        assert_eq!(
            resolved.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["¥", "12.50", "65.0", " °C"]
        );
        assert_eq!(
            resolved.iter().map(|r| r.font_scale).collect::<Vec<_>>(),
            [0.7, 1., 1., 0.5]
        );
        assert_eq!(resolved[0].style.font_size, Some(40.));
        let expired = resolve(
            &rich,
            &sensors,
            &account,
            &DeepSeekPreferences::default(),
            200000,
        );
        assert_eq!(
            expired.iter().map(|r| r.text.as_str()).collect::<Vec<_>>(),
            ["--", "--"]
        );
        assert!(expired.iter().all(|r| r.font_scale == 1.));
        let mut hidden = rich.clone();
        if let RichRun::Deepseek { show_currency, .. } = &mut hidden.runs[0] {
            *show_currency = false;
        }
        if let RichRun::Sensor { show_unit, .. } = &mut hidden.runs[1] {
            *show_unit = false;
        }
        assert_eq!(
            resolve(
                &hidden,
                &sensors,
                &account,
                &DeepSeekPreferences::default(),
                1000
            )
            .len(),
            2
        );
        if let RichRun::Sensor { affix_scale, .. } = &mut hidden.runs[1] {
            *affix_scale = Some(0.1);
        }
        assert!(hidden.validate().is_err());
    }
    #[test]
    fn all_metrics_follow_currency_and_do_not_add_composition_labels() {
        let account = deepseek::Snapshot {
            configured: true,
            status: "ok".into(),
            timestamp: 1000,
            balances: vec![
                deepseek::Balance {
                    currency: "CNY".into(),
                    total: "12.50".into(),
                    granted: "2.25".into(),
                    topped_up: "10.25".into(),
                    today_spent: Some("0.125".into()),
                    total_spent: Some("3.015".into()),
                    ..Default::default()
                },
                deepseek::Balance {
                    currency: "USD".into(),
                    total: "9.50".into(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let rich = RichText {
            version: 1,
            runs: [
                "balance", "granted", "toppedUp", "today", "total", "balance",
            ]
            .into_iter()
            .map(|metric| RichRun::Deepseek {
                metric: metric.into(),
                decimals: 2,
                show_currency: metric == "balance",
                affix_scale: None,
                style: RichStyle::default(),
            })
            .collect(),
        };
        let texts = resolve(&rich, &[], &account, &DeepSeekPreferences::default(), 1000)
            .into_iter()
            .map(|r| r.text)
            .collect::<Vec<_>>();
        assert_eq!(texts, ["¥12.50", "2.25", "10.25", "0.13", "3.02", "¥12.50"]);
        let prefs = DeepSeekPreferences {
            currency: "USD".into(),
            ..Default::default()
        };
        assert_eq!(resolve(&rich, &[], &account, &prefs, 1000)[0].text, "$9.50");
        assert_eq!(resolve(&rich, &[], &account, &prefs, 181001)[0].text, "--");
    }
    #[test]
    fn money_rounding_is_decimal_and_does_not_create_negative_zero() {
        for (value, decimals, expected) in [
            ("1.005", 2, "1.01"),
            ("9.99995", 4, "10.0000"),
            ("-1.005", 2, "-1.01"),
            ("-0.0001", 0, "0"),
            ("12", 8, "12.00000000"),
            ("1.123456789", 8, "1.12345679"),
            ("-0.000000005", 8, "-0.00000001"),
        ] {
            assert_eq!(rounded_money(value, decimals).as_deref(), Some(expected));
        }
    }
    #[test]
    fn mixed_fields_are_independent_and_expire_then_recover() {
        let sensor = SensorSample {
            id: "gpu-1".into(),
            name: "GPU 温度".into(),
            unit: "°C".into(),
            value: Some(65.25),
            timestamp: 1000,
            status: "ok".into(),
            source_id: 0,
            gpu_index: 0,
            device: "GPU".into(),
        };
        let mut account = deepseek::Snapshot {
            configured: true,
            status: "ok".into(),
            timestamp: 1000,
            balances: vec![deepseek::Balance {
                currency: "CNY".into(),
                total: "12.34565".into(),
                ..Default::default()
            }],
            ..Default::default()
        };
        let rich = RichText {
            version: 1,
            runs: vec![
                RichRun::Text {
                    text: "余额：".into(),
                    style: RichStyle::default(),
                },
                RichRun::Deepseek {
                    metric: "balance".into(),
                    decimals: 4,
                    show_currency: true,
                    affix_scale: None,
                    style: RichStyle::default(),
                },
                RichRun::Text {
                    text: "\nGPU：".into(),
                    style: RichStyle::default(),
                },
                RichRun::Sensor {
                    sensor_id: "gpu-1".into(),
                    label: "温度".into(),
                    decimals: 1,
                    show_unit: true,
                    affix_scale: None,
                    style: RichStyle::default(),
                },
                RichRun::Sensor {
                    sensor_id: "missing".into(),
                    label: "缺失".into(),
                    decimals: 0,
                    show_unit: false,
                    affix_scale: None,
                    style: RichStyle::default(),
                },
            ],
        };
        let join = |sensors: &[SensorSample], account: &deepseek::Snapshot, now| {
            resolve(
                &rich,
                sensors,
                account,
                &DeepSeekPreferences::default(),
                now,
            )
            .into_iter()
            .map(|r| r.text)
            .collect::<String>()
        };
        assert_eq!(
            join(&[sensor.clone()], &account, 1000),
            "余额：¥12.3457\nGPU：65.2 °C--"
        );
        assert_eq!(
            join(&[sensor.clone()], &account, 7000),
            "余额：¥12.3457\nGPU：----"
        );
        assert_eq!(
            join(&[sensor.clone()], &account, 182000),
            "余额：--\nGPU：----"
        );
        account.timestamp = 182000;
        let sensor = SensorSample {
            timestamp: 182000,
            ..sensor
        };
        assert_eq!(
            join(&[sensor], &account, 182000),
            "余额：¥12.3457\nGPU：65.2 °C--"
        );
    }
}
