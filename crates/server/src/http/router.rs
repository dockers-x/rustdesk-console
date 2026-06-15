//! Route table, ports of `http/router/{api,admin,router}.go`.

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::trace::TraceLayer;

use crate::http::{admin, api, static_files};
use crate::state::AppState;

pub fn build(state: AppState) -> Router {
    let web_client_enabled = state.config.app.web_client == 1;

    let cors = CorsLayer::new()
        .allow_origin(Any)
        .allow_methods(Any)
        .allow_headers(Any);

    let mut app = Router::new()
        // web index + config
        .route("/", get(static_files::index))
        // ---- client API (/api) ----
        .route("/api/", get(api::index))
        .route("/api/version", get(api::version))
        .route("/api/heartbeat", post(api::heartbeat))
        .route("/api/login-options", get(api::login_options))
        .route("/api/login", post(api::login))
        .route("/api/logout", post(api::logout))
        .route("/api/currentUser", post(api::user_info))
        .route("/api/user/info", get(api::user_info))
        .route("/api/sysinfo", post(api::sysinfo))
        .route("/api/sysinfo_ver", post(api::sysinfo_ver))
        .route("/api/users", get(api::group_users))
        .route("/api/peers", get(api::group_peers))
        .route("/api/device-group/accessible", get(api::device_group_accessible))
        .route("/api/audit/conn", post(api::audit_conn))
        .route("/api/audit/file", post(api::audit_file))
        // address book (legacy)
        .route("/api/ab", get(api::ab_get).post(api::ab_update))
        // address book (personal)
        .route("/api/ab/personal", post(api::ab_personal))
        .route("/api/ab/settings", post(api::ab_settings))
        .route("/api/ab/shared/profiles", post(api::ab_shared_profiles))
        .route("/api/ab/peers", post(api::ab_peers))
        .route("/api/ab/tags/:guid", post(api::ab_tags))
        .route("/api/ab/peer/add/:guid", post(api::ab_peer_add))
        .route("/api/ab/peer/:guid", delete(api::ab_peer_del))
        .route("/api/ab/peer/update/:guid", put(api::ab_peer_update))
        .route("/api/ab/tag/add/:guid", post(api::ab_tag_add))
        .route("/api/ab/tag/rename/:guid", put(api::ab_tag_rename))
        .route("/api/ab/tag/update/:guid", put(api::ab_tag_update))
        .route("/api/ab/tag/:guid", delete(api::ab_tag_del));

    if web_client_enabled {
        app = app
            .route("/api/shared-peer", post(api::shared_peer))
            .route("/api/server-config", post(api::server_config))
            .route("/api/server-config-v2", post(api::server_config_v2))
            .route("/webclient-config/index.js", get(static_files::config_js))
            .route("/webclient", get(static_files::webclient_index))
            .route("/webclient/", get(static_files::webclient_index))
            .route("/webclient/*path", get(static_files::webclient_path));
    }

    app = app.merge(admin_routes());

    // admin SPA (single-binary frontend)
    app = app
        .route("/_admin", get(static_files::admin_index))
        .route("/_admin/", get(static_files::admin_index))
        .route("/_admin/*path", get(static_files::admin_path));

    app.layer(cors)
        .layer(TraceLayer::new_for_http())
        .with_state(state)
}

fn admin_routes() -> Router<AppState> {
    Router::new()
        // auth (public)
        .route("/api/admin/login", post(admin::login))
        .route("/api/admin/captcha", get(admin::captcha))
        .route("/api/admin/logout", post(admin::logout))
        .route("/api/admin/login-options", get(admin::login_options))
        .route("/api/admin/oidc/auth", post(admin::not_implemented))
        .route("/api/admin/oidc/auth-query", get(admin::not_implemented))
        .route("/api/admin/user/register", post(admin::not_implemented))
        // config
        .route("/api/admin/config/admin", get(admin::config_admin))
        .route("/api/admin/config/server", get(admin::config_server))
        .route("/api/admin/config/app", get(admin::config_app))
        // user
        .route("/api/admin/user/current", get(admin::user_current))
        .route("/api/admin/user/changeCurPwd", post(admin::user_change_cur_pwd))
        .route("/api/admin/user/myOauth", post(admin::user_my_oauth))
        .route("/api/admin/user/groupUsers", post(admin::user_group_users))
        .route("/api/admin/user/list", get(admin::user_list))
        .route("/api/admin/user/detail/:id", get(admin::user_detail))
        .route("/api/admin/user/create", post(admin::user_create))
        .route("/api/admin/user/update", post(admin::user_update))
        .route("/api/admin/user/delete", post(admin::user_delete))
        .route("/api/admin/user/changePwd", post(admin::user_change_pwd))
        // group
        .route("/api/admin/group/list", get(admin::group_list))
        .route("/api/admin/group/detail/:id", get(admin::group_detail))
        .route("/api/admin/group/create", post(admin::group_create))
        .route("/api/admin/group/update", post(admin::group_update))
        .route("/api/admin/group/delete", post(admin::group_delete))
        // device group
        .route("/api/admin/device_group/list", get(admin::device_group_list))
        .route("/api/admin/device_group/detail/:id", get(admin::device_group_detail))
        .route("/api/admin/device_group/create", post(admin::device_group_create))
        .route("/api/admin/device_group/update", post(admin::device_group_update))
        .route("/api/admin/device_group/delete", post(admin::device_group_delete))
        // tag
        .route("/api/admin/tag/list", get(admin::tag_list))
        .route("/api/admin/tag/detail/:id", get(admin::tag_detail))
        .route("/api/admin/tag/create", post(admin::tag_create))
        .route("/api/admin/tag/update", post(admin::tag_update))
        .route("/api/admin/tag/delete", post(admin::tag_delete))
        // peer
        .route("/api/admin/peer/simpleData", post(admin::not_implemented))
        .route("/api/admin/peer/list", get(admin::peer_list))
        .route("/api/admin/peer/detail/:id", get(admin::peer_detail))
        .route("/api/admin/peer/create", post(admin::not_implemented))
        .route("/api/admin/peer/update", post(admin::peer_update))
        .route("/api/admin/peer/delete", post(admin::peer_delete))
        .route("/api/admin/peer/batchDelete", post(admin::peer_batch_delete))
        // login log (read implemented; deletes deferred)
        .route("/api/admin/login_log/list", get(admin::login_log_list))
        .route("/api/admin/login_log/delete", post(admin::not_implemented))
        .route("/api/admin/login_log/batchDelete", post(admin::not_implemented))
        // areas deferred to a later phase
        .route("/api/admin/oauth/list", get(admin::not_implemented))
        .route("/api/admin/audit_conn/list", get(admin::not_implemented))
        .route("/api/admin/audit_file/list", get(admin::not_implemented))
        .route("/api/admin/share_record/list", get(admin::not_implemented))
        .route("/api/admin/user_token/list", get(admin::not_implemented))
        .route("/api/admin/address_book/list", get(admin::not_implemented))
        .route("/api/admin/address_book_collection/list", get(admin::not_implemented))
        .route("/api/admin/address_book_collection_rule/list", get(admin::not_implemented))
        .route("/api/admin/rustdesk/cmdList", get(admin::not_implemented))
}
