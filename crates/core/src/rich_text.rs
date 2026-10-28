use crate::valid_color;
use anyhow::{ensure, Result};
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct RichStyle {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_family: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub font_size: Option<f32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub color: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub bold: Option<bool>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "lowercase", deny_unknown_fields)]
pub enum RichRun {
    Text {
        text: String,
        #[serde(default)]
        style: RichStyle,
    },
    Sensor {
        #[serde(rename = "sensorId")]
        sensor_id: String,
        label: String,
        decimals: u8,
        #[serde(rename = "showUnit")]
        show_unit: bool,
        #[serde(
            default,
            rename = "affixScale",
            skip_serializing_if = "Option::is_none"
        )]
        affix_scale: Option<f32>,
        #[serde(default)]
        style: RichStyle,
    },
    Deepseek {
        metric: String,
        decimals: u8,
        #[serde(rename = "showCurrency")]
        show_currency: bool,
        #[serde(
            default,
            rename = "affixScale",
            skip_serializing_if = "Option::is_none"
        )]
        affix_scale: Option<f32>,
        #[serde(default)]
        style: RichStyle,
    },
}
impl RichRun {
    pub fn affix_scale(&self) -> f32 {
        match self {
            Self::Sensor { affix_scale, .. } | Self::Deepseek { affix_scale, .. } => {
                affix_scale.unwrap_or(1.)
            }
            Self::Text { .. } => 1.,
        }
    }
    pub fn style(&self) -> &RichStyle {
        match self {
            Self::Text { style, .. }
            | Self::Sensor { style, .. }
            | Self::Deepseek { style, .. } => style,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RichText {
    pub version: u8,
    pub runs: Vec<RichRun>,
}
impl RichText {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.runs.len() <= 256,
            "混排文字版本或片段数量无效"
        );
        let mut size = 0;
        for run in &self.runs {
            ensure!(
                run.affix_scale().is_finite() && (0.25..=1.).contains(&run.affix_scale()),
                "单位或货币符号字号比例无效"
            );
            let style = run.style();
            ensure!(
                style
                    .font_size
                    .is_none_or(|v| v.is_finite() && (8. ..=240.).contains(&v)),
                "片段字号无效"
            );
            ensure!(
                style.color.as_ref().is_none_or(|v| valid_color(v)),
                "片段颜色无效"
            );
            ensure!(
                style
                    .font_family
                    .as_ref()
                    .is_none_or(|v| !v.is_empty() && v.len() <= 400),
                "片段字体无效"
            );
            match run {
                RichRun::Text { text, .. } => size += text.len(),
                RichRun::Sensor {
                    sensor_id,
                    label,
                    decimals,
                    ..
                } => {
                    ensure!(
                        !sensor_id.is_empty()
                            && sensor_id.len() <= 2048
                            && *decimals <= 8
                            && label.len() <= 600,
                        "传感器参数无效"
                    );
                    size += label.len();
                }
                RichRun::Deepseek {
                    metric, decimals, ..
                } => {
                    ensure!(
                        ["balance", "granted", "toppedUp", "today", "total", "period"]
                            .contains(&metric.as_str())
                            && *decimals <= 8,
                        "DeepSeek 参数无效"
                    );
                }
            }
        }
        ensure!(size <= 16000, "混排文字过长");
        let summary_size: usize = self
            .runs
            .iter()
            .map(|run| match run {
                RichRun::Text { text, .. } => text.len(),
                RichRun::Sensor {
                    sensor_id, label, ..
                } => {
                    if label.is_empty() {
                        sensor_id.len() + 2
                    } else {
                        label.len() + 2
                    }
                }
                RichRun::Deepseek { metric, .. } => {
                    "[DeepSeek · ]".len()
                        + match metric.as_str() {
                            "balance" => "余额",
                            "granted" => "赠送余额",
                            "toppedUp" => "充值余额",
                            "today" => "今日消费估算",
                            "total" => "累计消费估算",
                            _ => "峰谷时段",
                        }
                        .len()
                }
            })
            .sum();
        ensure!(summary_size <= 16000, "混排文字摘要过长");
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{Layer, LayerKind};
    #[test]
    fn old_layers_load_and_rich_styles_roundtrip_without_a_config_version_change() {
        let original = Layer::new(LayerKind::Text);
        let mut old = serde_json::to_value(&original).unwrap();
        old.as_object_mut().unwrap().remove("verticalAlign");
        assert!(old.get("richText").is_none());
        let mut loaded: Layer = serde_json::from_value(old).unwrap();
        assert!(loaded.rich_text.is_none());
        assert_eq!(loaded.vertical_align, "middle");
        loaded.rich_text = Some(RichText {
            version: 1,
            runs: vec![RichRun::Text {
                text: "温度\n余额".into(),
                style: RichStyle {
                    font_size: Some(48.),
                    bold: Some(false),
                    color: Some("#ffcc00".into()),
                    ..Default::default()
                },
            }],
        });
        loaded.validate().unwrap();
        assert_eq!(
            loaded,
            serde_json::from_slice::<Layer>(&serde_json::to_vec(&loaded).unwrap()).unwrap()
        );
    }
    #[test]
    fn validation_bounds_untrusted_rich_content() {
        for (text, style) in [
            ("字".repeat(5334), RichStyle::default()),
            (
                "x".into(),
                RichStyle {
                    font_size: Some(241.),
                    ..Default::default()
                },
            ),
            (
                "x".into(),
                RichStyle {
                    color: Some("red".into()),
                    ..Default::default()
                },
            ),
        ] {
            assert!(RichText {
                version: 1,
                runs: vec![RichRun::Text { text, style }]
            }
            .validate()
            .is_err());
        }
        let run = RichRun::Deepseek {
            metric: "balance".into(),
            decimals: 4,
            show_currency: true,
            affix_scale: None,
            style: RichStyle::default(),
        };
        assert!(RichText {
            version: 1,
            runs: vec![run; 257]
        }
        .validate()
        .is_err());
        assert!(RichText {
            version: 2,
            runs: vec![]
        }
        .validate()
        .is_err());
        assert!(serde_json::from_str::<RichText>(
            r#"{"version":1,"runs":[{"type":"text","text":"a","script":"no"}]}"#
        )
        .is_err());
    }
}
