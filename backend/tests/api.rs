mod support;

use backend::{
    models::VouchersCreateRequest,
    unleashed::session::SessionError,
    unleashed_api::{ApiConfig, UnleashedAPI, startup_retry_delay},
};
use reqwest::StatusCode;
use support::{Mock, PASSWORD, USERNAME};

async fn api(mock: &Mock) -> UnleashedAPI {
    UnleashedAPI::try_new(ApiConfig {
        url: mock.url.clone(),
        username: USERNAME.into(),
        password: PASSWORD.into(),
        ssid: "Guest".into(),
        verify_tls: true,
        timezone: chrono_tz::Tz::UTC,
        rolling_voucher_duration_minutes: 480,
    })
    .await
    .unwrap()
}

fn request(count: u32, name: &str) -> VouchersCreateRequest {
    VouchersCreateRequest {
        count,
        name: name.into(),
        authorized_guest_limit: None,
        time_limit_minutes: 1440,
        code: None,
        remarks: None,
    }
}

#[tokio::test]
async fn startup_fails_on_bad_credentials() {
    let mock = Mock::start().await;
    let result = UnleashedAPI::try_new(ApiConfig {
        url: mock.url.clone(),
        username: USERNAME.into(),
        password: "wrong".into(),
        ssid: "Guest".into(),
        verify_tls: true,
        timezone: chrono_tz::Tz::UTC,
        rolling_voucher_duration_minutes: 480,
    })
    .await;
    assert!(matches!(result, Err(SessionError::Auth(_))));
}

#[test]
fn startup_retries_back_off_to_five_minutes() {
    let delays: Vec<u64> = (0..9).map(|a| startup_retry_delay(a).as_secs()).collect();
    assert_eq!(delays, vec![5, 10, 20, 40, 80, 160, 300, 300, 300]);
    assert_eq!(startup_retry_delay(u32::MAX).as_secs(), 300);
}

#[tokio::test]
async fn creates_a_single_pass_with_a_spaced_name() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let created = api
        .create_voucher(&request(1, "Quick Voucher"))
        .await
        .unwrap();
    assert_eq!(created.vouchers.len(), 1);
    let v = &created.vouchers[0];
    assert_eq!(v.name, "Quick-Voucher");
    assert_eq!(v.time_limit_minutes, 1440);
    let form = &mock.state.lock().unwrap().create_forms[0];
    assert_eq!(form["duration"], "1");
    assert_eq!(form["duration-unit"], "day");
    assert_eq!(form["guest-wlan"], "Guest");
}

#[tokio::test]
async fn duplicate_names_are_fine() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    api.create_voucher(&request(1, "Quick Voucher"))
        .await
        .unwrap();
    api.create_voucher(&request(1, "Quick Voucher"))
        .await
        .unwrap();
    assert_eq!(api.get_all_vouchers().await.unwrap().data.len(), 2);
}

#[tokio::test]
async fn creates_a_batch() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    mock.add_pass("existing", false, 3600);
    let created = api.create_voucher(&request(3, "ignored")).await.unwrap();
    assert_eq!(
        created.vouchers.len(),
        3,
        "only the new passes are returned"
    );
}

#[tokio::test]
async fn custom_key_and_remarks() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let created = api
        .create_voucher(&VouchersCreateRequest {
            code: Some("front1".into()),
            remarks: Some("desk".into()),
            authorized_guest_limit: Some(3),
            ..request(1, "desk")
        })
        .await
        .unwrap();
    let v = &created.vouchers[0];
    assert_eq!(v.code, "FRONT1");
    assert_eq!(v.remarks, "desk");
    assert_eq!(v.authorized_guest_limit, Some(3));
}

#[tokio::test]
async fn duplicate_key_is_a_conflict() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let with_key = VouchersCreateRequest {
        code: Some("SAME1".into()),
        ..request(1, "a")
    };
    api.create_voucher(&with_key).await.unwrap();
    assert_eq!(
        api.create_voucher(&with_key).await.unwrap_err(),
        StatusCode::CONFLICT
    );
}

#[tokio::test]
async fn invalid_requests_are_bad_requests() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    for bad in [
        request(0, "a"),
        request(101, "a"),
        VouchersCreateRequest {
            time_limit_minutes: 0,
            ..request(1, "a")
        },
        VouchersCreateRequest {
            code: Some("AB".into()),
            ..request(2, "a")
        },
        VouchersCreateRequest {
            code: Some("a b".into()),
            ..request(1, "a")
        },
    ] {
        assert_eq!(
            api.create_voucher(&bad).await.unwrap_err(),
            StatusCode::BAD_REQUEST
        );
    }
    assert!(mock.state.lock().unwrap().create_forms.is_empty());
}

#[tokio::test]
async fn an_ok_that_creates_nothing_is_an_error() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    mock.state.lock().unwrap().ignore_creates = true;
    assert_eq!(
        api.create_voucher(&request(1, "a")).await.unwrap_err(),
        StatusCode::BAD_GATEWAY
    );
}

#[tokio::test]
async fn lists_newest_first_and_filters_by_name() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    api.create_voucher(&request(1, "alpha")).await.unwrap();
    api.create_voucher(&request(1, "Beta")).await.unwrap();
    let all = api.get_all_vouchers().await.unwrap();
    assert_eq!(all.total_count, 2);
    let filtered = api
        .get_vouchers(&backend::models::VouchersGetRequest {
            offset: 0,
            limit: 10,
            filter: Some("BET".into()),
        })
        .await
        .unwrap();
    assert_eq!(filtered.data.len(), 1);
    assert_eq!(filtered.data[0].name, "Beta");
}

#[tokio::test]
async fn details_and_newest() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let id = mock.add_pass("one", false, 3600).to_string();
    assert_eq!(api.get_voucher_details(id).await.unwrap().name, "one");
    assert_eq!(
        api.get_voucher_details("999".into()).await.unwrap_err(),
        StatusCode::NOT_FOUND
    );
    assert_eq!(api.get_newest_voucher().await.unwrap().name, "one");
}

#[tokio::test]
async fn deletes_selected_passes() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let a = mock.add_pass("a", false, 3600);
    let b = mock.add_pass("b", false, 3600);
    let c = mock.add_pass("c", false, 3600);
    let deleted = api
        .delete_vouchers_by_ids(vec![a.to_string(), c.to_string(), "999".into()])
        .await
        .unwrap();
    assert_eq!(deleted.vouchers_deleted, 2, "missing ids are not counted");
    assert_eq!(mock.pass_ids(), vec![b]);
}

#[tokio::test]
async fn rejects_non_numeric_ids() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let a = mock.add_pass("a", false, 3600);
    let injection = "1'></guest><guest id='2".to_string();
    assert_eq!(
        api.delete_vouchers_by_ids(vec![injection])
            .await
            .unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    assert_eq!(mock.pass_ids(), vec![a]);
}

#[tokio::test]
async fn deleting_nothing_is_fine() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    assert_eq!(
        api.delete_vouchers_by_ids(vec!["".into()])
            .await
            .unwrap()
            .vouchers_deleted,
        0
    );
    assert_eq!(
        api.delete_expired_vouchers()
            .await
            .unwrap()
            .vouchers_deleted,
        0
    );
    assert_eq!(
        api.delete_expired_rolling_vouchers()
            .await
            .unwrap()
            .vouchers_deleted,
        0
    );
}

#[tokio::test]
async fn rolling_vouchers_roll() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    assert!(api.get_rolling_voucher().await.unwrap().is_none());

    let first = api.create_rolling_voucher().await.unwrap();
    assert!(first.name.starts_with("[ROLLING]-"));
    assert!(!first.name.contains("192.0.2"), "no address in the name");
    assert_eq!(first.time_limit_minutes, 480);
    assert_eq!(
        api.get_rolling_voucher().await.unwrap().unwrap().id,
        first.id
    );

    // While it waits unused, any further request gets the same voucher back
    let again = api.create_rolling_voucher().await.unwrap();
    assert_eq!(again.id, first.id);
    assert_eq!(mock.pass_ids().len(), 1);

    // A guest uses the first pass; the next one becomes current.
    {
        let mut s = mock.state.lock().unwrap();
        let pass = s
            .passes
            .iter_mut()
            .find(|p| p.id.to_string() == first.id)
            .unwrap();
        pass.client_macs.push("00:00:00:00:00:09".into());
    }
    assert!(api.get_rolling_voucher().await.unwrap().is_none());
    let second = api.create_rolling_voucher().await.unwrap();
    assert_eq!(
        api.get_rolling_voucher().await.unwrap().unwrap().id,
        second.id
    );
}

#[tokio::test]
async fn survives_a_controller_reboot() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    mock.expire_sessions();
    assert!(api.get_all_vouchers().await.is_ok());
    assert_eq!(mock.logins(), 2);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn concurrent_creates_return_only_their_own_pass() {
    let mock = Mock::start().await;
    mock.state.lock().unwrap().create_delay_ms = 100;
    let api = std::sync::Arc::new(api(&mock).await);
    let calls: Vec<_> = ["one", "two"]
        .into_iter()
        .map(|name| {
            let api = api.clone();
            tokio::spawn(async move { api.create_voucher(&request(1, name)).await })
        })
        .collect();
    for (call, name) in calls.into_iter().zip(["one", "two"]) {
        let created = call.await.unwrap().unwrap();
        let names: Vec<&str> = created.vouchers.iter().map(|v| v.name.as_str()).collect();
        assert_eq!(names, vec![name], "a create returned another create's pass");
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn overlapping_rolling_requests_share_one_voucher() {
    let mock = Mock::start().await;
    mock.state.lock().unwrap().create_delay_ms = 100;
    let api = std::sync::Arc::new(api(&mock).await);
    let calls: Vec<_> = (0..2)
        .map(|_| {
            let api = api.clone();
            tokio::spawn(async move { api.create_rolling_voucher().await })
        })
        .collect();
    let mut ids = Vec::new();
    for call in calls {
        ids.push(call.await.unwrap().unwrap().id);
    }
    assert_eq!(ids[0], ids[1], "both requests should get the same voucher");
    assert_eq!(mock.pass_ids().len(), 1);
}

#[tokio::test]
async fn the_kiosk_gets_a_new_voucher_after_its_own_is_used() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    // The kiosk mints the first rolling voucher under its own address
    let first = api.create_rolling_voucher().await.unwrap();
    // A guest uses it, but /welcome never loads
    {
        let mut s = mock.state.lock().unwrap();
        let pass = s
            .passes
            .iter_mut()
            .find(|p| p.id.to_string() == first.id)
            .unwrap();
        pass.client_macs.push("00:00:00:00:00:09".into());
    }
    // The kiosk's next check creates a fresh one instead of being refused
    let next = api.create_rolling_voucher().await.unwrap();
    assert_ne!(next.id, first.id);
}

#[tokio::test]
async fn names_and_remarks_the_controller_refuses_are_bad_requests() {
    let mock = Mock::start().await;
    let api = api(&mock).await;
    let name = request(1, "Bob's phone");
    assert_eq!(
        api.create_voucher(&name).await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    let remarks = VouchersCreateRequest {
        remarks: Some("a<b".into()),
        ..request(1, "desk")
    };
    assert_eq!(
        api.create_voucher(&remarks).await.unwrap_err(),
        StatusCode::BAD_REQUEST
    );
    assert!(mock.state.lock().unwrap().create_forms.is_empty());
}
