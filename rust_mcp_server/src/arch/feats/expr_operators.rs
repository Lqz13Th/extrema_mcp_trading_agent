use polars::prelude::*;

use extrema_infra::prelude::*;

pub const EPSILON: f64 = 1e-8_f64;

pub fn collect_schema_safe(lf: &LazyFrame) -> InfraResult<Arc<Schema>> {
    Ok(lf.clone().collect_schema()?)
}

pub fn convert_all_to_float64_except_timestamp(lf: LazyFrame) -> InfraResult<LazyFrame> {
    let schema = collect_schema_safe(&lf)?;

    let exprs: Vec<_> = schema
        .iter()
        .filter_map(|(name, dtype)| {
            if name == "timestamp" {
                None
            } else if *dtype != DataType::Float64 {
                Some(col(name.as_str()).cast(DataType::Float64))
            } else {
                None
            }
        })
        .collect();

    Ok(lf.with_columns(exprs))
}

pub fn z_score_expr(col_name: &str, window: usize) -> Expr {
    let (mean_expr, std_expr) = rolling_mean_std_expr(col_name, window);
    normalize_clip_expr(col_name, mean_expr, std_expr)
        .alias(format!("z_{}", col_name))
}


pub fn rolling_mean_std_expr(col_name: &str, window: usize) -> (Expr, Expr) {
    let mean_expr = col(col_name).rolling_mean(RollingOptionsFixedWindow {
        window_size: window,
        min_periods: 1,
        center: false,
        ..Default::default()
    });
    let std_expr = col(col_name).rolling_std(RollingOptionsFixedWindow {
        window_size: window,
        min_periods: 1,
        center: false,
        ..Default::default()
    }).fill_nan(lit(0.0));
    (mean_expr, std_expr)
}

pub fn normalize_clip_expr(col_name: &str, mean_expr: Expr, std_expr: Expr) -> Expr {
    ((col(col_name) - mean_expr) / (std_expr + lit(EPSILON)))
        .fill_nan(lit(0.0))
        .fill_null(lit(0.0))
        .clip(lit(-3.0), lit(3.0))
}