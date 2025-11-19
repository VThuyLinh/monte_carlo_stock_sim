use serde::Deserialize;
use anyhow::{Result, bail};
use nalgebra::DVector;
use chrono::NaiveDate;

// 1. Cấu trúc dữ liệu
#[derive(Debug, Deserialize, Clone)]
pub struct StockRecord {
    // SỬA: Đổi tên field thành <Ticker>
    #[serde(rename = "<Ticker>")]
    pub ticker: String,
    // SỬA: Đổi tên field thành <DTYYYYMMDD> và sử dụng format ngày mới
    #[serde(rename = "<DTYYYYMMDD>", with = "csv_date_format")]
    pub date: NaiveDate,
    
    // SỬA: Đổi tên các field khác sang dạng có dấu ngoặc nhọn
    #[serde(rename = "<Open>")]
    _open: f64,
    #[serde(rename = "<High>")]
    _high: f64,
    #[serde(rename = "<Low>")]
    _low: f64,
    #[serde(rename = "<Close>")]
    pub close: f64,
    #[serde(rename = "<Volume>")]
    _volume: u64,
}

// Giúp Slint/GUI hiển thị thông tin ước tính
pub struct ParameterEstimate {
    pub mean_log_return_daily: f64,
    pub std_dev_log_return_daily: f64,
    pub mean_log_return_annualized: f64,
    pub std_dev_log_return_annualized: f64,
}

pub fn get_available_tickers(file_path: &str) -> Result<Vec<String>> {
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b',')
        .from_path(file_path)?;

    // Sử dụng BTreeSet để đảm bảo các ticker là duy nhất và được sắp xếp
    let mut tickers = std::collections::BTreeSet::new();
    
    // Đọc từng bản ghi, chỉ lấy trường <Ticker>
    for result in reader.records() {
        let record = result?;
        // Giả sử <Ticker> là trường đầu tiên (index 0)
        if let Some(ticker) = record.get(0) {
            tickers.insert(ticker.to_string());
        }
    }

    if tickers.is_empty() {
        bail!("Không tìm thấy Ticker nào trong file: {}", file_path);
    }
    
    Ok(tickers.into_iter().collect())
}

// Module để parse NaiveDate từ CSV
mod csv_date_format {
    use chrono::NaiveDate;
    use serde::{self, Deserialize, Deserializer};

    // SỬA: Format ngày là YYYYMMDD
    const DATE_FORMAT: &str = "%Y%m%d";

    pub fn deserialize<'de, D>(
        deserializer: D,
    ) -> Result<NaiveDate, D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        // Parse ngày với format mới
        NaiveDate::parse_from_str(&s, DATE_FORMAT).map_err(serde::de::Error::custom)
    }
}

// 2. Hàm đọc CSV và lọc theo Ticker
pub fn load_and_filter_data(file_path: &str, target_ticker: &str) -> Result<Vec<StockRecord>> {
    // Không cần thay đổi logic hàm này
    let mut reader = csv::ReaderBuilder::new()
        .delimiter(b',')
        .from_path(file_path)?;

    let mut filtered_data = Vec::new();
    for result in reader.deserialize() {
        let record: StockRecord = result?;
        // Lỗi này có thể xảy ra nếu bạn cố gắng dùng Ticker mặc định là AAPL
        // trong khi data của bạn dùng AAA/AAM/AAT.
        if record.ticker == target_ticker {
            filtered_data.push(record);
        }
    }

    if filtered_data.is_empty() {
        bail!("Không tìm thấy dữ liệu cho Ticker: {}", target_ticker);
    }
    
    // Sắp xếp theo ngày để đảm bảo tính toán log-return đúng
    filtered_data.sort_by_key(|r| r.date);
    
    Ok(filtered_data)
}

// 3. Hàm tính Log-Returns
fn compute_log_returns(prices: &[f64]) -> Vec<f64> {
    prices.iter()
        .zip(prices.iter().skip(1))
        .map(|(prev, current)| {
            (current / prev).ln() // r_t = ln(Close_t / Close_{t-1})
        })
        .collect()
}

// 4. Hàm ước tính Parameters (μ và σ)
pub fn estimate_parameters(data: &[StockRecord]) -> Result<ParameterEstimate> {
    if data.len() < 2 {
        bail!("Không đủ dữ liệu (ít hơn 2 bản ghi) để tính log-returns.");
    }
    
    // Lấy chuỗi giá đóng cửa
    let prices: Vec<f64> = data.iter().map(|r| r.close).collect();
    // Tính toán log-returns
    let returns = compute_log_returns(&prices);

    let n = returns.len();
    if n == 0 {
        bail!("Không có log-returns nào được tính toán.");
    }

    // Sử dụng nalgebra để tính Mean và Std Dev dễ dàng
    let returns_vector = DVector::from_vec(returns);
    
    // Tính daily parameters
    let mean_daily = returns_vector.sum() / (n as f64);
    let std_dev_daily = (returns_vector.iter()
        .map(|r| (r - mean_daily).powi(2))
        .sum::<f64>() / ((n - 1) as f64)
    ).sqrt();

    // Chuẩn hóa sang năm (Giả sử 252 ngày giao dịch/năm)
    const TRADING_DAYS_PER_YEAR: f64 = 252.0;
    
    let mean_annualized = mean_daily * TRADING_DAYS_PER_YEAR;
    let std_dev_annualized = std_dev_daily * TRADING_DAYS_PER_YEAR.sqrt();

    Ok(ParameterEstimate {
        mean_log_return_daily: mean_daily,
        std_dev_log_return_daily: std_dev_daily,
        mean_log_return_annualized: mean_annualized,
        std_dev_log_return_annualized: std_dev_annualized,
    })
}