//! Route table, ports of `http/router/{api,admin,router}.go`.

use axum::routing::{delete, get, post, put};
use axum::Router;
use tower_http::cors::{Any, CorsLayer};
use tower_http::services::ServeDir;
use tower_http::trace::TraceLayer;

use crate::http::{admin, api, file, my, oauth, static_files};
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
        // oauth / oidc login
        .route("/api/oidc/auth", post(oauth::oidc_auth))
        .route("/api/oidc/auth-query", get(oauth::oidc_auth_query))
        .route("/api/oidc/callback", get(oauth::oauth_callback))
        .route("/api/oidc/login", get(oauth::oauth_callback))
        .route("/api/oidc/msg", get(oauth::message))
        .route("/api/oauth/callback", get(oauth::oauth_callback))
        .route("/api/oauth/login", get(oauth::oauth_callback))
        .route("/api/oauth/msg", get(oauth::message))
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
            .route("/webclient/*path", get(static_files::webclient_path))
            .route("/webclient2", get(static_files::webclient_index))
            .route("/webclient2/", get(static_files::webclient_index))
            .route("/webclient2/*path", get(static_files::webclient_path));
    }

    app = app.merge(admin_routes());

    // admin SPA (single-binary frontend)
    app = app
        .route("/_admin", get(static_files::admin_index))
        .route("/_admin/", get(static_files::admin_index))
        .route("/_admin/*path", get(static_files::admin_path));

    // user-uploaded files (written to disk under resources/public/upload)
    let upload_dir = {
        let base = if state.config.gin.resources_path.is_empty() {
            "resources".to_string()
        } else {
            state.config.gin.resources_path.clone()
        };
        format!("{base}/public/upload")
    };
    app = app.nest_service("/upload", ServeDir::new(upload_dir));

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
        .route("/api/admin/oidc/auth", post(oauth::admin_oidc_auth))
        .route("/api/admin/oidc/auth-query", get(oauth::admin_oidc_auth_query))
        .route("/api/admin/user/register", post(admin::user_register))
        // config
        .route("/api/admin/config/admin", get(admin::config_admin))
        .route(
            "/api/admin/config/server",
            get(admin::config_server).patch(admin::config_server_update),
        )
        .route("/api/admin/config/app", get(admin::config_app))
        // user
        .route("/api/admin/user/current", get(admin::user_current))
        .route("/api/admin/user/changeCurPwd", post(admin::user_change_cur_pwd))
        .route("/api/admin/user/myOauth", post(admin::user_my_oauth_real))
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
        .route("/api/admin/peer/simpleData", post(admin::peer_simple_data))
        .route("/api/admin/peer/list", get(admin::peer_list))
        .route("/api/admin/peer/detail/:id", get(admin::peer_detail))
        .route("/api/admin/peer/create", post(admin::peer_create))
        .route("/api/admin/peer/update", post(admin::peer_update))
        .route("/api/admin/peer/delete", post(admin::peer_delete))
        .route("/api/admin/peer/batchDelete", post(admin::peer_batch_delete))
        // login log
        .route("/api/admin/login_log/list", get(admin::login_log_list))
        .route("/api/admin/login_log/delete", post(admin::login_log_delete))
        .route("/api/admin/login_log/batchDelete", post(admin::login_log_batch_delete))
        // oauth providers
        .route("/api/admin/oauth/list", get(admin::oauth_list))
        .route("/api/admin/oauth/detail/:id", get(admin::oauth_detail))
        .route("/api/admin/oauth/create", post(admin::oauth_create))
        .route("/api/admin/oauth/update", post(admin::oauth_update))
        .route("/api/admin/oauth/delete", post(admin::oauth_delete))
        .route("/api/admin/oauth/unbind", post(admin::oauth_unbind))
        .route("/api/admin/oauth/confirm", post(oauth::admin_confirm))
        .route("/api/admin/oauth/bind", post(oauth::admin_to_bind))
        .route("/api/admin/oauth/bindConfirm", post(oauth::admin_bind_confirm))
        .route("/api/admin/oauth/info", get(oauth::admin_info))
        // audit
        .route("/api/admin/audit_conn/list", get(admin::audit_conn_list))
        .route("/api/admin/audit_conn/delete", post(admin::audit_conn_delete))
        .route("/api/admin/audit_conn/batchDelete", post(admin::audit_conn_batch_delete))
        .route("/api/admin/audit_file/list", get(admin::audit_file_list))
        .route("/api/admin/audit_file/delete", post(admin::audit_file_delete))
        .route("/api/admin/audit_file/batchDelete", post(admin::audit_file_batch_delete))
        // share records
        .route("/api/admin/share_record/list", get(admin::share_record_list))
        .route("/api/admin/share_record/delete", post(admin::share_record_delete))
        .route("/api/admin/share_record/batchDelete", post(admin::share_record_batch_delete))
        // user tokens
        .route("/api/admin/user_token/list", get(admin::user_token_list))
        .route("/api/admin/user_token/delete", post(admin::user_token_delete))
        .route("/api/admin/user_token/batchDelete", post(admin::user_token_batch_delete))
        // address book
        .route("/api/admin/address_book/list", get(admin::address_book_list))
        .route("/api/admin/address_book/detail/:id", get(admin::address_book_detail))
        .route("/api/admin/address_book/create", post(admin::address_book_create))
        .route("/api/admin/address_book/update", post(admin::address_book_update))
        .route("/api/admin/address_book/delete", post(admin::address_book_delete))
        .route("/api/admin/address_book/batchCreate", post(admin::address_book_batch_create))
        .route(
            "/api/admin/address_book/batchCreateFromPeers",
            post(admin::address_book_batch_create_from_peers),
        )
        .route("/api/admin/address_book/shareByWebClient", post(admin::address_book_share))
        // address book collections
        .route("/api/admin/address_book_collection/list", get(admin::collection_list))
        .route("/api/admin/address_book_collection/detail/:id", get(admin::collection_detail))
        .route("/api/admin/address_book_collection/create", post(admin::collection_create))
        .route("/api/admin/address_book_collection/update", post(admin::collection_update))
        .route("/api/admin/address_book_collection/delete", post(admin::collection_delete))
        // address book collection rules
        .route("/api/admin/address_book_collection_rule/list", get(admin::rule_list))
        .route("/api/admin/address_book_collection_rule/detail/:id", get(admin::rule_detail))
        .route("/api/admin/address_book_collection_rule/create", post(admin::rule_create))
        .route("/api/admin/address_book_collection_rule/update", post(admin::rule_update))
        .route("/api/admin/address_book_collection_rule/delete", post(admin::rule_delete))
        // rustdesk server commands
        .route("/api/admin/rustdesk/cmdList", get(admin::rustdesk_cmd_list))
        .route("/api/admin/rustdesk/cmdCreate", post(admin::rustdesk_cmd_create))
        .route("/api/admin/rustdesk/cmdUpdate", post(admin::rustdesk_cmd_update))
        .route("/api/admin/rustdesk/cmdDelete", post(admin::rustdesk_cmd_delete))
        .route("/api/admin/rustdesk/sendCmd", post(admin::rustdesk_send_cmd))
        // file upload (local + OSS)
        .route("/api/admin/file/upload", post(file::upload))
        .route("/api/admin/file/oss_token", get(file::oss_token))
        .route("/api/admin/file/notify", post(file::notify))
        // my/*
        .route("/api/admin/my/share_record/list", get(my::share_record_list))
        .route("/api/admin/my/share_record/delete", post(my::share_record_delete))
        .route("/api/admin/my/share_record/batchDelete", post(my::share_record_batch_delete))
        .route("/api/admin/my/address_book/list", get(my::address_book_list))
        .route("/api/admin/my/address_book/create", post(my::address_book_create))
        .route("/api/admin/my/address_book/update", post(my::address_book_update))
        .route("/api/admin/my/address_book/delete", post(my::address_book_delete))
        .route(
            "/api/admin/my/address_book/batchCreateFromPeers",
            post(my::address_book_batch_create_from_peers),
        )
        .route("/api/admin/my/address_book/batchUpdateTags", post(my::address_book_batch_update_tags))
        .route("/api/admin/my/tag/list", get(my::tag_list))
        .route("/api/admin/my/tag/create", post(my::tag_create))
        .route("/api/admin/my/tag/update", post(my::tag_update))
        .route("/api/admin/my/tag/delete", post(my::tag_delete))
        .route("/api/admin/my/address_book_collection/list", get(my::collection_list))
        .route("/api/admin/my/address_book_collection/create", post(my::collection_create))
        .route("/api/admin/my/address_book_collection/update", post(my::collection_update))
        .route("/api/admin/my/address_book_collection/delete", post(my::collection_delete))
        .route("/api/admin/my/address_book_collection_rule/list", get(my::rule_list))
        .route("/api/admin/my/address_book_collection_rule/create", post(my::rule_create))
        .route("/api/admin/my/address_book_collection_rule/update", post(my::rule_update))
        .route("/api/admin/my/address_book_collection_rule/delete", post(my::rule_delete))
        .route("/api/admin/my/peer/list", get(my::peer_list))
        .route("/api/admin/my/login_log/list", get(my::login_log_list))
        .route("/api/admin/my/login_log/delete", post(my::login_log_delete))
        .route("/api/admin/my/login_log/batchDelete", post(my::login_log_batch_delete))
}
