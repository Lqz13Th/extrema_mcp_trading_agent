use polars::prelude::*;

use extrema_infra::{
    prelude::*,
    arch::market_assets::api_data::utils_data::*,
};

pub fn oi_to_lf(oi: Vec<OpenInterest>) -> InfraResult<LazyFrame> {
    let ts: Vec<u64> = oi.iter().map(|x| x.timestamp).collect();
    let sum_oi: Vec<f64> = oi.iter().map(|x| x.sum_open_interest).collect();
    let sum_oi_val: Vec<f64> = oi
        .iter()
        .map(|x| x.sum_open_interest_value.unwrap_or_default())
        .collect();

    let mut df = df![
        "timestamp" => ts,
        "sum_open_interest" => sum_oi,
        "sum_open_interest_value" => sum_oi_val,
    ]?;

    df.rename("sum_open_interest", "oi_sum_open_interest".into())?;
    df.rename("sum_open_interest_value", "oi_sum_open_interest_value".into())?;

    Ok(df.lazy())
}