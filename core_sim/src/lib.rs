
use rand::{SeedableRng, Rng};
use rand::rngs::StdRng;
use rand_distr::{StandardNormal, Distribution};
use anyhow::Result;

// Cấu trúc chứa kết quả của 1 lần chạy mô phỏng
#[derive(Clone)]
pub struct SimulationResult {
    // Lưu trữ một số đường đi (paths) mẫu (ví dụ: 50 đường)
    pub sample_paths: Vec<Vec<f64>>,
    // Lưu trữ giá kết thúc (terminal prices) của TẤT CẢ paths
    pub terminal_prices: Vec<f64>,
}

// 1. Mô phỏng Geometric Brownian Motion (GBM)
pub fn simulate_gbm(
    initial_price: f64,
    daily_mu: f64,
    daily_sigma: f64,
    horizon_days: usize,
    num_paths: usize,
    dt: f64, // dt (time step) = 1.0 / steps_per_day
    seed: u64,
    antithetic: bool, // Antithetic Variates
) -> Result<SimulationResult> {
    
    // Khởi tạo bộ sinh số ngẫu nhiên với seed
    let mut rng = StdRng::seed_from_u64(seed);
    
    // Số bước thời gian (steps)
    let steps = (horizon_days as f64 / dt).round() as usize;
    if steps == 0 {
        return Err(anyhow::anyhow!("Horizon Days / dt quá nhỏ, dẫn đến số bước = 0"));
    }
    
    let paths_to_generate = if antithetic {
        num_paths / 2
    } else {
        num_paths
    };

    let mut all_terminal_prices = Vec::with_capacity(num_paths);
    let mut sample_paths = Vec::new();
    let sample_path_limit = 50; // Giới hạn số đường đi mẫu để lưu

    // Các tham số trong công thức GBM
    let mu_dt = daily_mu * dt;
    let sigma_sqrt_dt = daily_sigma * dt.sqrt();
    let drift_term_per_step = mu_dt - 0.5 * daily_sigma.powi(2) * dt;

    for path_index in 0..paths_to_generate {
        // --- Path 1 (Sử dụng Z) ---
        let mut path1 = Vec::with_capacity(steps + 1);
        let mut current_price1 = initial_price;
        path1.push(current_price1);

        for _ in 0..steps {
            // Lấy số ngẫu nhiên Z ~ N(0, 1)
            let z: f64 = rng.sample(StandardNormal);
            // S_{t+dt} = S_t * exp((mu - 0.5*sigma^2)*dt + sigma*sqrt(dt)*Z)
            current_price1 *= (drift_term_per_step + sigma_sqrt_dt * z).exp();
            path1.push(current_price1);
        }
        
        all_terminal_prices.push(*path1.last().unwrap_or(&initial_price));
        if path_index < sample_path_limit {
            // Chỉ lưu trữ một số đường đi mẫu
            sample_paths.push(path1);
        }

        // --- Path 2 (Sử dụng -Z cho Antithetic Variates) ---
        if antithetic {
            // Thiết lập lại rng cho path thứ 2 để sử dụng cùng một sequence Z, nhưng sau đó nhân với -1
            // Cách đơn giản hơn: chạy lại quá trình sinh random như Path 1, nhưng nhân Z với -1
            
            // Do cần sử dụng cùng một chuỗi Z nhưng ngược dấu, ta phải lưu lại chuỗi Z hoặc 
            // đảm bảo rng.sample(StandardNormal) trả về giá trị đối xứng.
            
            // Với StdRng, việc sinh chuỗi Z như sau sẽ đảm bảo Z cho Path 1 và Z cho Path 2 đối xứng
            // nếu ta sinh 2*paths_to_generate số Z.
            
            let mut path2 = Vec::with_capacity(steps + 1);
            let mut current_price2 = initial_price;
            path2.push(current_price2);

            // Path 2 sử dụng Antithetic Variates (-Z)
            for _ in 0..steps {
                let z: f64 = rng.sample(StandardNormal); // Dùng số ngẫu nhiên tiếp theo
                current_price2 *= (drift_term_per_step - sigma_sqrt_dt * z).exp();
                path2.push(current_price2);
            }
            
            all_terminal_prices.push(*path2.last().unwrap_or(&initial_price));
            if path_index + paths_to_generate < sample_path_limit { // Kiểm tra chỉ số path2
                sample_paths.push(path2);
            }
        }
    }

    Ok(SimulationResult {
        sample_paths,
        terminal_prices: all_terminal_prices,
    })
}

// 2. Mô phỏng Historical Bootstrap
pub fn simulate_bootstrap(
    initial_price: f64,
    historical_returns: &[f64],
    horizon_days: usize,
    num_paths: usize,
    seed: u64,
) -> Result<SimulationResult> {
    
    if historical_returns.is_empty() {
        return Err(anyhow::anyhow!("Historical returns không được rỗng."));
    }
    
    let mut rng = StdRng::seed_from_u64(seed);
    let n_returns = historical_returns.len();
    
    let mut all_terminal_prices = Vec::with_capacity(num_paths);
    let mut sample_paths = Vec::new();
    let sample_path_limit = 50; 

    for path_index in 0..num_paths {
        let mut path = Vec::with_capacity(horizon_days + 1);
        let mut current_price = initial_price;
        path.push(current_price);

        for _ in 0..horizon_days {
            // Chọn ngẫu nhiên một log-return từ lịch sử
            let random_index = rng.gen_range(0..n_returns);
            let random_return = historical_returns[random_index];
            
            // S_t = S_{t-1} * exp(r_t)
            current_price *= random_return.exp();
            path.push(current_price);
        }
        
        all_terminal_prices.push(*path.last().unwrap_or(&initial_price));
        if path_index < sample_path_limit {
            sample_paths.push(path);
        }
    }

    Ok(SimulationResult {
        sample_paths,
        terminal_prices: all_terminal_prices,
    })
}

// 3. Hàm tính toán Metrics (sẽ được gọi từ app_gui)
pub fn calculate_metrics(prices: &[f64], initial_price: f64) -> Result<Metrics> {
    if prices.is_empty() {
        return Err(anyhow::anyhow!("Không có giá kết thúc để tính toán metrics."));
    }

    // Tính lợi nhuận tổng thể
    let mut total_returns: Vec<f64> = prices.iter()
        .map(|p| (p / initial_price) - 1.0)
        .collect();

    // Sắp xếp để tính percentile
    total_returns.sort_by(|a, b| a.partial_cmp(b).unwrap_or(std::cmp::Ordering::Equal));
    
    let n = prices.len() as f64;
    
    let mean = total_returns.iter().sum::<f64>() / n;
    
    // Standard Deviation
    let std_dev = (total_returns.iter()
        .map(|r| (r - mean).powi(2))
        .sum::<f64>() / (n - 1.0)
    ).sqrt();

    // Các Percentile
    let p5_index = ((0.05 * n).round() as usize).min(total_returns.len() - 1);
    let p25_index = ((0.25 * n).round() as usize).min(total_returns.len() - 1);
    let p50_index = ((0.50 * n).round() as usize).min(total_returns.len() - 1); // Median
    let p75_index = ((0.75 * n).round() as usize).min(total_returns.len() - 1);
    let p95_index = ((0.95 * n).round() as usize).min(total_returns.len() - 1);

    let p5 = total_returns[p5_index];
    let p25 = total_returns[p25_index];
    let median = total_returns[p50_index];
    let p75 = total_returns[p75_index];
    let p95 = total_returns[p95_index];
    
    // VaR95 (Value at Risk 95%) = negative của P5 (lợi nhuận)
    // VaR95: -P5% của lợi nhuận, thể hiện khoản lỗ tối đa ở mức tin cậy 95%
    let var95 = -p5 * initial_price; // Tính theo giá trị tiền tệ
    let var95_percent = -p5 * 100.0; // Tính theo phần trăm

    Ok(Metrics {
        mean_return: mean,
        std_dev_return: std_dev,
        median_return: median,
        p5_return: p5,
        p25_return: p25,
        p75_return: p75,
        p95_return: p95,
        var95_percent,
        var95_value: var95,
    })
}

pub struct Metrics {
    pub mean_return: f64,
    pub std_dev_return: f64,
    pub median_return: f64,
    pub p5_return: f64,
    pub p25_return: f64,
    pub p75_return: f64,
    pub p95_return: f64,
    pub var95_percent: f64,
    pub var95_value: f64,
}

// pub fn add(left: u64, right: u64) -> u64 {
//     left + right
// }

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn it_works() {
//         let result = add(2, 2);
//         assert_eq!(result, 4);
//     }
// }
