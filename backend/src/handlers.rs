use std::net::IpAddr;

use axum::{
    extract::Query,
    http::{HeaderMap, StatusCode},
    response::Json,
};
use tracing::{debug, error};

use crate::{models::*, unleashed_api::UNLEASHED_API};

pub async fn get_vouchers_filtered_handler(
    Query(params): Query<VouchersGetRequest>,
) -> Result<Json<VouchersGetResponse>, StatusCode> {
    debug!("Received request to get vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_vouchers(&params).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to get vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn get_all_vouchers_handler() -> Result<Json<VouchersGetResponse>, StatusCode> {
    debug!("Received request to get all vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_all_vouchers().await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to get vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn get_rolling_voucher_handler() -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to get rolling voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_rolling_voucher().await {
        Ok(Some(voucher)) => Ok(Json(voucher)),
        Ok(None) => Err(StatusCode::NOT_FOUND),
        Err(e) => {
            error!("Failed to get rolling voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn get_newest_voucher_handler() -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to get newest voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_newest_voucher().await {
        Ok(voucher) => Ok(Json(voucher)),
        Err(e) => {
            error!("Failed to get newest voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn get_voucher_details_handler(
    Query(params): Query<VoucherDetailsRequest>,
) -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to get voucher details");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.get_voucher_details(params.id).await {
        Ok(voucher) => Ok(Json(voucher)),
        Err(e) => {
            error!("Failed to get voucher details: {}", e);
            Err(e)
        }
    }
}

pub async fn create_voucher_handler(
    Json(request): Json<VouchersCreateRequest>,
) -> Result<Json<VouchersCreateResponse>, StatusCode> {
    debug!("Received request to create voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.create_voucher(&request).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to create voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn create_rolling_voucher_handler(
    headers: HeaderMap,
) -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request to create voucher");

    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");

    if let Some(forwarded) = headers.get("x-forwarded-for")
        && let Ok(forwarded) = forwarded.to_str()
        && let Some(ip) = forwarded_client_ip(forwarded)
    {
        let ip = ip.to_string();
        let ip = ip.as_str();
        debug!("Client IP from x-forwarded-for: {}", ip);

        // One rolling voucher per IP; the API answers FORBIDDEN otherwise
        match client.create_rolling_voucher(ip).await {
            Ok(response) => return Ok(Json(response)),
            Err(StatusCode::FORBIDDEN) => return Err(StatusCode::FORBIDDEN),
            Err(e) => {
                error!("Failed to create rolling voucher: {}", e);
                return Err(e);
            }
        }
    }

    error!("Invalid x-forwarded-for header");
    Err(StatusCode::BAD_REQUEST)
}

pub async fn delete_selected_handler(
    Query(params): Query<VouchersDeleteRequest>,
) -> Result<Json<DeleteResponse>, StatusCode> {
    debug!("Received request to delete selected vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    let ids = params.ids.split(',').map(|s| s.to_string()).collect();
    match client.delete_vouchers_by_ids(ids).await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to delete selected vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn delete_expired_handler() -> Result<Json<DeleteResponse>, StatusCode> {
    debug!("Received request to delete expired vouchers");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.delete_expired_vouchers().await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to delete expired vouchers: {}", e);
            Err(e)
        }
    }
}

pub async fn delete_expired_rolling_handler() -> Result<Json<DeleteResponse>, StatusCode> {
    debug!("Received request to delete expired rolling voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    match client.delete_expired_rolling_vouchers().await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to delete expired rolling voucher: {}", e);
            Err(e)
        }
    }
}

pub async fn health_check_handler() -> Result<Json<HealthCheckResponse>, StatusCode> {
    debug!("Received health check request");
    let response = HealthCheckResponse {
        status: "ok".to_string(),
    };
    Ok(Json(response))
}

/// The client address the frontend forwards in `X-Forwarded-For`: the first
/// entry, which must parse as an IP address. The frontend puts the address
/// the reverse proxy saw there; anything else is rejected, so arbitrary text
/// never reaches a pass name.
pub fn forwarded_client_ip(header: &str) -> Option<IpAddr> {
    let first = header.split(',').next()?.trim();
    match first.parse().ok()? {
        IpAddr::V6(v6) => Some(v6.to_ipv4_mapped().map_or(IpAddr::V6(v6), IpAddr::V4)),
        v4 => Some(v4),
    }
}
