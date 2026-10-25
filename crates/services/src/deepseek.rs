use anyhow::{ensure, Context, Result};
use chrono::{Datelike, TimeZone, Timelike, Utc};
use pixelferry_core::DeepSeekPreferences;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    collections::BTreeMap,
    io::Read,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
pub const URL: &str = "https://api.deepseek.com/user/balance";
const SCALE: i128 = 100_000_000;
pub fn day(now: i64) -> String {
    Utc.timestamp_millis_opt(now + 8 * 3_600_000)
        .unwrap()
        .format("%Y-%m-%d")
        .to_string()
}
pub fn units(value: &str) -> Result<i128> {
    ensure!(!value.is_empty() && value.len() <= 30, "金额无效");
    let negative = value.starts_with('-');
    let value = value.strip_prefix('-').unwrap_or(value);
    let parts: Vec<_> = value.split('.').collect();
    ensure!(
        parts.len() <= 2 && !parts[0].is_empty() && parts[0].bytes().all(|b| b.is_ascii_digit()),
        "金额无效"
    );
    let fraction = parts.get(1).copied().unwrap_or("");
    ensure!(
        fraction.len() <= 12
            && (parts.len() == 1 || !fraction.is_empty())
            && fraction.bytes().all(|b| b.is_ascii_digit()),
        "金额小数无效"
    );
    let mut padded = format!("{fraction:0<8}");
    padded.truncate(8);
    let whole: i128 = parts[0].parse()?;
    let n = whole
        .checked_mul(SCALE)
        .and_then(|n| n.checked_add(padded.parse::<i128>().ok()?))
        .context("金额超限")?;
    Ok(if negative { -n } else { n })
}
pub fn money(n: i128) -> String {
    format!(
        "{}{}.{:08}",
        if n < 0 { "-" } else { "" },
        n.unsigned_abs() / SCALE as u128,
        n.unsigned_abs() % SCALE as u128
    )
}
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase")]
pub struct Record {
    pub last: String,
    pub timestamp: i64,
    pub baseline_at: i64,
    pub total: String,
    pub days: BTreeMap<String, String>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ledger {
    pub version: u32,
    pub accounts: BTreeMap<String, BTreeMap<String, Record>>,
}
impl Default for Ledger {
    fn default() -> Self {
        Self {
            version: 1,
            accounts: BTreeMap::new(),
        }
    }
}
impl Ledger {
    pub fn validate(&self) -> Result<()> {
        ensure!(
            self.version == 1 && self.accounts.len() <= 10000,
            "消费记录格式无效"
        );
        for currencies in self.accounts.values() {
            for (currency, r) in currencies {
                ensure!(
                    ["CNY", "USD"].contains(&currency.as_str())
                        && r.timestamp >= 0
                        && r.timestamp <= 4_102_444_800_000
                        && r.baseline_at >= 0
                        && r.baseline_at <= 4_102_444_800_000
                        && r.days.len() <= 400,
                    "消费记录无效"
                );
                for n in [&r.last, &r.total].into_iter().chain(r.days.values()) {
                    let _: i128 = n.parse().context("消费记录金额无效")?;
                }
                ensure!(
                    r.total.parse::<i128>()? >= 0
                        && r.days
                            .values()
                            .all(|v| v.parse::<i128>().is_ok_and(|n| n >= 0)),
                    "消费累计值不能为负"
                );
            }
        }
        Ok(())
    }
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceResponse {
    pub is_available: bool,
    pub balance_infos: Vec<BalanceInfo>,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BalanceInfo {
    pub currency: String,
    pub total_balance: String,
    pub granted_balance: String,
    pub topped_up_balance: String,
}
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Balance {
    pub currency: String,
    pub total: String,
    pub granted: String,
    pub topped_up: String,
    pub today_spent: Option<String>,
    pub total_spent: Option<String>,
    pub baseline_at: i64,
}
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Snapshot {
    pub configured: bool,
    pub status: String,
    pub timestamp: i64,
    pub available: bool,
    pub balances: Vec<Balance>,
    pub error: Option<String>,
    pub statistics_error: Option<String>,
}
impl Default for Snapshot {
    fn default() -> Self {
        Self {
            configured: false,
            status: "unconfigured".into(),
            timestamp: 0,
            available: false,
            balances: vec![],
            error: None,
            statistics_error: None,
        }
    }
}
pub fn record(
    ledger: &Ledger,
    account: &str,
    response: &BalanceResponse,
    now: i64,
) -> Result<(Ledger, Vec<Balance>)> {
    ensure!(response.balance_infos.len() <= 2, "余额响应无效");
    let mut next = ledger.clone();
    let records = next.accounts.entry(account.into()).or_default();
    let today = day(now);
    let mut seen = std::collections::BTreeSet::new();
    let mut balances = vec![];
    for b in &response.balance_infos {
        ensure!(
            ["CNY", "USD"].contains(&b.currency.as_str()) && seen.insert(&b.currency),
            "余额币种无效"
        );
        let current = units(&b.total_balance)?;
        units(&b.granted_balance)?;
        units(&b.topped_up_balance)?;
        let previous = records.get(&b.currency);
        let mut days = previous.map(|p| p.days.clone()).unwrap_or_default();
        let mut total = previous
            .map(|p| p.total.parse::<i128>())
            .transpose()?
            .unwrap_or(0);
        if let Some(p) = previous {
            let delta = p
                .last
                .parse::<i128>()?
                .checked_sub(current)
                .context("消费差额超限")?;
            if delta > 0 {
                total = total.checked_add(delta).context("累计消费超限")?;
                if day(p.timestamp) == today {
                    let spent = days
                        .get(&today)
                        .map(|s| s.parse::<i128>())
                        .transpose()?
                        .unwrap_or(0)
                        .checked_add(delta)
                        .context("每日消费超限")?;
                    days.insert(today.clone(), spent.to_string());
                }
            }
        }
        while days.len() > 400 {
            days.pop_first();
        }
        let baseline_at = previous.map(|p| p.baseline_at).unwrap_or(now);
        balances.push(Balance {
            currency: b.currency.clone(),
            total: b.total_balance.clone(),
            granted: b.granted_balance.clone(),
            topped_up: b.topped_up_balance.clone(),
            today_spent: Some(money(
                days.get(&today)
                    .map(|s| s.parse::<i128>())
                    .transpose()?
                    .unwrap_or(0),
            )),
            total_spent: Some(money(total)),
            baseline_at,
        });
        records.insert(
            b.currency.clone(),
            Record {
                last: current.to_string(),
                timestamp: now,
                baseline_at,
                total: total.to_string(),
                days,
            },
        );
    }
    Ok((next, balances))
}
pub fn peak(now: i64, p: &DeepSeekPreferences) -> bool {
    let t = Utc.timestamp_millis_opt(now + 8 * 3_600_000).unwrap();
    let weekday = t.weekday().number_from_monday() <= 5;
    let minute = t.hour() * 60 + t.minute();
    weekday
        && !p.holidays.contains(&day(now))
        && ((540..720).contains(&minute) || (840..1080).contains(&minute))
}
pub fn period(now: i64, p: &DeepSeekPreferences) -> (bool, bool, Option<i64>, Option<i64>) {
    let current = peak(now, p);
    let local = Utc.timestamp_millis_opt(now + 8 * 3_600_000).unwrap();
    let midnight = local
        .date_naive()
        .and_hms_opt(0, 0, 0)
        .unwrap()
        .and_utc()
        .timestamp_millis()
        - 8 * 3_600_000;
    let mut start = None;
    let mut next = None;
    for d in 0..15 {
        for minute in [0, 540, 720, 840, 1080] {
            let c = midnight + d * 86_400_000 + minute * 60_000;
            if next.is_none() && c > now && peak(c, p) != current {
                next = Some(c);
            }
        }
        for minute in [1080, 840, 720, 540, 0] {
            let c = midnight - d * 86_400_000 + minute * 60_000;
            if start.is_none() && c <= now && peak(c, p) == current && peak(c - 1, p) != current {
                start = Some(c);
            }
        }
    }
    (
        current,
        p.calendar_years.contains(&local.year()),
        start,
        next,
    )
}
pub fn progress(now: i64, p: &DeepSeekPreferences) -> f64 {
    let (_, _, a, b) = period(now, p);
    match (a, b) {
        (Some(a), Some(b)) if b > a => ((now - a) as f64 / (b - a) as f64).clamp(0., 1.),
        _ => 0.,
    }
}
pub fn text(metric: &str, s: &Snapshot, p: &DeepSeekPreferences, now: i64) -> String {
    let (peak, known, _, _) = period(now, p);
    let period = format!(
        "{}{}",
        if peak { "峰时段" } else { "谷时段" },
        if known { "" } else { " · 节假日待核对" }
    );
    if metric == "period" {
        return period;
    }
    let b = s.balances.iter().find(|b| b.currency == p.currency);
    let amount = |field: &str| -> String {
        if s.status != "ok" || now - s.timestamp > 180000 {
            return "--".into();
        }
        let v = b.and_then(|b| match field {
            "today" => b.today_spent.as_deref(),
            "total" => b.total_spent.as_deref(),
            _ => Some(b.total.as_str()),
        });
        let v = if field == "today" && day(s.timestamp) != day(now) {
            Some("0")
        } else {
            v
        };
        v.and_then(|s| s.parse::<f64>().ok())
            .filter(|n| n.is_finite())
            .map(|n| format!("{}{n:.4}", if p.currency == "CNY" { "¥" } else { "$" }))
            .unwrap_or("--".into())
    };
    match metric {
        "balance" => amount("balance"),
        "today" => format!("今日估算 {}", amount("today")),
        "total" => format!("累计估算 {}", amount("total")),
        _ => format!(
            "DeepSeek · {period}\n余额 {}\n今日估算 {}\n累计估算 {}",
            amount("balance"),
            amount("today"),
            amount("total")
        ),
    }
}
pub struct Service {
    root: PathBuf,
    key: String,
    ledger: Ledger,
    pub snapshot: Snapshot,
    failures: u32,
    blocked: bool,
    next: Instant,
    ledger_valid: bool,
}
impl Service {
    pub fn new(root: &Path) -> Self {
        let mut snapshot = Snapshot::default();
        let ledger = match std::fs::read(root.join("deepseek-ledger.json")) {
            Ok(bytes) => serde_json::from_slice::<Ledger>(&bytes)
                .and_then(|l| l.validate().map(|_| l).map_err(serde::de::Error::custom)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(Ledger::default()),
            Err(e) => Err(serde::de::Error::custom(e)),
        };
        let ledger_valid = ledger.is_ok();
        if !ledger_valid {
            snapshot.statistics_error = Some("消费记录无法读取，原文件已保留，暂不统计".into());
        }
        let key = match std::fs::read(root.join("deepseek-key.bin")) {
            Ok(bytes) => match crate::secrets::decrypt(&bytes) {
                Ok(key) => key,
                Err(_) => {
                    snapshot.status = "error".into();
                    snapshot.error = Some("无法解密 Key，请重新输入".into());
                    String::new()
                }
            },
            Err(_) => String::new(),
        };
        snapshot.configured = !key.is_empty();
        Self {
            root: root.into(),
            key,
            ledger: ledger.unwrap_or_default(),
            snapshot,
            failures: 0,
            blocked: false,
            next: Instant::now(),
            ledger_valid,
        }
    }
    pub fn change_key(&mut self, key: Option<&str>) -> Result<()> {
        if let Some(key) = key {
            let encrypted = crate::secrets::encrypt(key.trim())?;
            atomic(&self.root.join("deepseek-key.bin"), &encrypted)?;
            self.key = key.trim().into();
        } else {
            match std::fs::remove_file(self.root.join("deepseek-key.bin")) {
                Ok(()) => {}
                Err(e) if e.kind() == std::io::ErrorKind::NotFound => {}
                Err(e) => return Err(e.into()),
            }
            self.key.clear();
        }
        self.snapshot = Snapshot {
            configured: !self.key.is_empty(),
            ..Default::default()
        };
        self.blocked = false;
        self.failures = 0;
        self.next = Instant::now();
        Ok(())
    }
    pub fn refresh(&mut self) {
        self.blocked = false;
        self.next = Instant::now();
    }
    pub fn tick(&mut self) {
        if self.key.is_empty() || self.blocked || Instant::now() < self.next {
            return;
        }
        let now = crate::now();
        match self.poll() {
            Ok(response) => match record(
                &self.ledger,
                &format!("{:x}", Sha256::digest(self.key.as_bytes())),
                &response,
                now,
            ) {
                Ok((ledger, mut balances)) => {
                    if self.ledger_valid {
                        match serde_json::to_vec(&ledger)
                            .map_err(anyhow::Error::from)
                            .and_then(|b| atomic(&self.root.join("deepseek-ledger.json"), &b))
                        {
                            Ok(()) => self.ledger = ledger,
                            Err(_) => {
                                self.ledger_valid = false;
                                self.snapshot.statistics_error =
                                    Some("消费记录写入失败，暂不统计".into());
                            }
                        }
                    }
                    if !self.ledger_valid {
                        for b in &mut balances {
                            b.today_spent = None;
                            b.total_spent = None;
                        }
                    }
                    self.snapshot = Snapshot {
                        configured: true,
                        status: "ok".into(),
                        timestamp: now,
                        available: response.is_available,
                        balances,
                        error: None,
                        statistics_error: self.snapshot.statistics_error.clone(),
                    };
                    self.failures = 0;
                }
                Err(_) => self.error("余额数据异常"),
            },
            Err(PollError::Auth) => {
                self.blocked = true;
                self.error("API Key 无效或无访问权限");
            }
            Err(PollError::Other) => self.error("余额查询失败或超时，稍后重试"),
        };
        self.next =
            Instant::now() + Duration::from_secs((60u64 * 2u64.pow(self.failures.min(3))).min(300));
    }
    fn error(&mut self, message: &str) {
        self.failures += 1;
        self.snapshot.status = "error".into();
        self.snapshot.error = Some(message.into());
    }
    fn poll(&self) -> std::result::Result<BalanceResponse, PollError> {
        let tls = native_tls::TlsConnector::new().map_err(|_| PollError::Other)?;
        let agent = ureq::AgentBuilder::new()
            .tls_connector(std::sync::Arc::new(tls))
            .timeout(Duration::from_secs(10))
            .redirects(0)
            .build();
        let response = match agent
            .get(URL)
            .set("Authorization", &format!("Bearer {}", self.key))
            .set("Accept", "application/json")
            .call()
        {
            Ok(r) => r,
            Err(ureq::Error::Status(401 | 403, _)) => return Err(PollError::Auth),
            Err(_) => return Err(PollError::Other),
        };
        let mut bytes = vec![];
        response
            .into_reader()
            .take(65537)
            .read_to_end(&mut bytes)
            .map_err(|_| PollError::Other)?;
        if bytes.len() > 65536 {
            return Err(PollError::Other);
        }
        serde_json::from_slice(&bytes).map_err(|_| PollError::Other)
    }
}
enum PollError {
    Auth,
    Other,
}
pub fn atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    std::fs::create_dir_all(path.parent().context("无父目录")?)?;
    let temp = path.with_extension("tmp");
    {
        use std::io::Write;
        let mut f = std::fs::File::create(&temp)?;
        f.write_all(bytes)?;
        f.sync_all()?;
    }
    #[cfg(windows)]
    {
        use windows_sys::Win32::Storage::FileSystem::{
            MoveFileExW, MOVEFILE_REPLACE_EXISTING, MOVEFILE_WRITE_THROUGH,
        };
        let w = |p: &Path| {
            p.as_os_str()
                .to_string_lossy()
                .encode_utf16()
                .chain(Some(0))
                .collect::<Vec<_>>()
        };
        ensure!(
            unsafe {
                MoveFileExW(
                    w(&temp).as_ptr(),
                    w(path).as_ptr(),
                    MOVEFILE_REPLACE_EXISTING | MOVEFILE_WRITE_THROUGH,
                )
            } != 0,
            "原子保存失败"
        );
    }
    #[cfg(not(windows))]
    std::fs::rename(&temp, path)?;
    Ok(())
}
#[cfg(test)]
mod tests {
    use super::*;
    fn response(total: &str) -> BalanceResponse {
        BalanceResponse {
            is_available: true,
            balance_infos: vec![BalanceInfo {
                currency: "CNY".into(),
                total_balance: total.into(),
                granted_balance: "0".into(),
                topped_up_balance: total.into(),
            }],
        }
    }
    #[test]
    fn no_negative_spend_and_no_cross_day_assignment() {
        let t = Utc
            .with_ymd_and_hms(2026, 10, 26, 0, 0, 0)
            .unwrap()
            .timestamp_millis();
        let (l, _) = record(&Ledger::default(), "a", &response("10"), t).unwrap();
        let (l, b) = record(&l, "a", &response("9"), t + 1000).unwrap();
        assert_eq!(b[0].today_spent.as_deref(), Some("1.00000000"));
        let (l, b) = record(&l, "a", &response("20"), t + 2000).unwrap();
        assert_eq!(b[0].total_spent.as_deref(), Some("1.00000000"));
        let (_, b) = record(&l, "a", &response("19"), t + 86400000).unwrap();
        assert_eq!(b[0].today_spent.as_deref(), Some("0.00000000"));
        assert_eq!(b[0].total_spent.as_deref(), Some("2.00000000"));
    }
    #[test]
    fn continuous_valley_crosses_midnight() {
        let p = DeepSeekPreferences::default();
        let t = Utc
            .with_ymd_and_hms(2026, 10, 26, 15, 0, 0)
            .unwrap()
            .timestamp_millis();
        assert_eq!(period(t, &p).2, period(t + 2 * 3600000, &p).2);
    }
    #[test]
    fn fixed_precision() {
        assert_eq!(units("1.123456789").unwrap(), 112345678);
        assert!(units("NaN").is_err());
    }
}
