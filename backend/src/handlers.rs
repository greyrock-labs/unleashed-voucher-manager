use axum::{extract::Query, http::StatusCode, response::Json};
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

pub async fn create_rolling_voucher_handler() -> Result<Json<Voucher>, StatusCode> {
    debug!("Received request for the next rolling voucher");
    let client = UNLEASHED_API.get().expect("UnleashedAPI not initialized");
    // Returns the waiting rolling voucher, creating one only if none waits
    match client.create_rolling_voucher().await {
        Ok(response) => Ok(Json(response)),
        Err(e) => {
            error!("Failed to create rolling voucher: {}", e);
            Err(e)
        }
    }
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
