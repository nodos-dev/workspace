use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::Command;
use std::thread;
use std::time::Duration;

use chrono::{DateTime, Utc};
use reqwest::blocking::{Client, Response};
use reqwest::header::{AUTHORIZATION, CONTENT_TYPE, HeaderMap, HeaderValue};
use serde::{Deserialize, Serialize};

use crate::nosman::common::get_hostname;
use crate::nosman::index::{PackageReleaseEntry, PackageType, SemVer};
use crate::nosman::package::PackageIdentifier;

#[derive(Deserialize)]
struct ApiErrorResponse {
    message: String,
}

#[derive(Deserialize)]
struct PackageListResponse {
    packages: Vec<PackageSummaryResponse>,
    has_next_page: bool,
}

#[derive(Deserialize)]
struct PackageDetailResponse {
    package: PackageSummaryResponse,
}

#[derive(Clone, Deserialize)]
pub struct PackageSummaryResponse {
    pub name: String,
    pub package_type: String,
}

#[derive(Deserialize)]
struct ReleaseListResponse {
    releases: Vec<ReleaseSummaryResponse>,
    has_next_page: bool,
}

#[derive(Clone, Deserialize)]
pub struct ReleaseSummaryResponse {
    pub id: i64,
    pub version: String,
    pub tags: Vec<String>,
    pub artifacts: Vec<ReleaseArtifactResponse>,
    pub api_version: Option<ApiVersion>,
    #[serde(default)]
    pub dependencies: Vec<PackageIdentifier>,
    pub updated_at: String,
}

#[derive(Clone, Deserialize)]
pub struct ReleaseArtifactResponse {
    pub id: i64,
    pub target_platform: String,
}

#[derive(Clone, Deserialize, Serialize)]
pub struct ApiVersion {
    pub major: u32,
    pub minor: Option<u32>,
    pub patch: Option<u32>,
}

#[derive(Serialize)]
struct DeviceStartRequest {
    client_name: String,
}

#[derive(Deserialize)]
struct DeviceStartResponse {
    device_code: String,
    user_code: String,
    verification_uri: String,
    verification_uri_complete: String,
    expires_at: String,
    interval_seconds: u64,
}

#[derive(Serialize)]
struct DevicePollRequest {
    device_code: String,
}

#[derive(Deserialize)]
struct AccountMeResponse {
    needs_onboarding: bool,
}

#[derive(Deserialize)]
struct DevicePollResponse {
    access_token: Option<String>,
}

#[derive(Deserialize)]
struct DownloadTokenResponse {
    download_url: String,
}

#[derive(Serialize)]
struct PublishDraftArtifactRequest {
    target_platform: String,
}

#[derive(Serialize)]
struct PublishSessionCreateRequest {
    name: String,
    display_name: String,
    description: String,
    package_type: PackageType,
    category: String,
    version: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    api_version: Option<ApiVersion>,
    #[serde(default)]
    dependencies: Vec<PackageIdentifier>,
    #[serde(default)]
    tags: Vec<String>,
    #[serde(default)]
    artifacts: Vec<PublishDraftArtifactRequest>,
}

#[derive(Deserialize)]
struct PublishSessionResponse {
    session: PublishSessionSummary,
}

#[derive(Deserialize)]
struct PublishSessionSummary {
    id: i64,
}

#[derive(Serialize)]
struct PublishArtifactUploadRequest {
    target_platform: String,
}

#[derive(Deserialize)]
struct PublishUploadUrlResponse {
    upload_url: String,
}

#[derive(Deserialize)]
struct PublishFinalizeResponse {
    release: ReleaseSummaryResponse,
}

#[derive(Serialize, Deserialize)]
struct StoredAccessToken {
    access_token: String,
}

pub struct PublishReleaseRequest {
    pub name: String,
    pub display_name: String,
    pub description: String,
    pub package_type: PackageType,
    pub category: String,
    pub version: String,
    pub api_version: Option<SemVer>,
    pub dependencies: Vec<PackageIdentifier>,
    pub tags: Vec<String>,
    pub target_platform: String,
    pub artifact_path: PathBuf,
}

enum TokenSource {
    Env(String),
    Saved(String),
    None,
}

fn client(token: Option<&str>) -> Result<Client, String> {
    let mut headers = HeaderMap::new();
    if let Some(token) = token {
        let value = HeaderValue::from_str(&format!("Bearer {}", token))
            .map_err(|e| format!("Failed to build authorization header: {}", e))?;
        headers.insert(AUTHORIZATION, value);
    }
    Client::builder()
        .default_headers(headers)
        .build()
        .map_err(|e| format!("Failed to create HTTP client: {}", e))
}

fn api_url(base_url: &str, path: &str) -> String {
    format!(
        "{}/api/v1{}",
        base_url.trim_end_matches('/'),
        path
    )
}

fn read_error(response: Response) -> String {
    let status = response.status();
    let body = response.text().unwrap_or_default();
    if let Ok(error) = serde_json::from_str::<ApiErrorResponse>(&body) {
        return error.message;
    }
    if body.is_empty() {
        return format!("Request failed with status {}", status);
    }
    format!("Request failed with status {}: {}", status, body)
}

fn get_json<T: for<'de> Deserialize<'de>>(base_url: &str, path: &str, token: Option<&str>) -> Result<T, String> {
    let response = client(token)?
        .get(api_url(base_url, path))
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;
    if !response.status().is_success() {
        return Err(read_error(response));
    }
    response
        .json::<T>()
        .map_err(|e| format!("Failed to parse response: {}", e))
}

fn post_json<T: for<'de> Deserialize<'de>, B: Serialize>(base_url: &str, path: &str, body: &B, token: Option<&str>) -> Result<T, String> {
    let response = client(token)?
        .post(api_url(base_url, path))
        .json(body)
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;
    if !response.status().is_success() {
        return Err(read_error(response));
    }
    response
        .json::<T>()
        .map_err(|e| format!("Failed to parse response: {}", e))
}

fn post_empty(base_url: &str, path: &str, token: Option<&str>) -> Result<Response, String> {
    client(token)?
        .post(api_url(base_url, path))
        .send()
        .map_err(|e| format!("Request failed: {}", e))
}

fn delete(base_url: &str, path: &str, token: Option<&str>) -> Result<(), String> {
    let response = client(token)?
        .delete(api_url(base_url, path))
        .send()
        .map_err(|e| format!("Request failed: {}", e))?;
    if !response.status().is_success() {
        return Err(read_error(response));
    }
    Ok(())
}

fn artifact_reference_url(base_url: &str, artifact_id: i64) -> String {
    format!(
        "{}/api/v1/release-artifacts/{}",
        base_url.trim_end_matches('/'),
        artifact_id
    )
}

fn token_store_path() -> Option<PathBuf> {
    let base_dir = if cfg!(windows) {
        env::var_os("APPDATA").map(PathBuf::from)
    } else {
        env::var_os("XDG_CONFIG_HOME")
            .map(PathBuf::from)
            .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))
    }?;
    Some(
        base_dir
            .join("nosman")
            .join("package-server")
            .join("access-token.json"),
    )
}

fn load_saved_token() -> Option<String> {
    let path = token_store_path()?;
    let contents = fs::read_to_string(path).ok()?;
    serde_json::from_str::<StoredAccessToken>(&contents)
        .ok()
        .map(|stored| stored.access_token)
}

fn save_token(token: &str) -> Result<(), String> {
    let Some(path) = token_store_path() else {
        return Ok(());
    };
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .map_err(|e| format!("Failed to create token directory: {}", e))?;
    }
    let contents = serde_json::to_string(&StoredAccessToken {
        access_token: token.to_string(),
    })
    .map_err(|e| format!("Failed to serialize access token: {}", e))?;
    fs::write(path, contents).map_err(|e| format!("Failed to store access token: {}", e))
}

fn env_token() -> Option<String> {
    env::var("NOSMAN_PACKAGE_SERVER_TOKEN")
        .ok()
        .or_else(|| env::var("NODOS_STORE_ACCESS_TOKEN").ok())
}

fn validate_token(base_url: &str, token: &str) -> Result<AccountMeResponse, String> {
    get_json(base_url, "/account/me", Some(token))
}

fn open_browser(url: &str) {
    #[cfg(target_os = "windows")]
    let _ = Command::new("cmd").args(["/C", "start", "", url]).spawn();

    #[cfg(target_os = "macos")]
    let _ = Command::new("open").arg(url).spawn();

    #[cfg(all(unix, not(target_os = "macos")))]
    let _ = Command::new("xdg-open").arg(url).spawn();
}

fn start_device_login(base_url: &str) -> Result<String, String> {
    let start: DeviceStartResponse = post_json(
        base_url,
        "/auth/device/start",
        &DeviceStartRequest {
            client_name: format!("nosman on {}", get_hostname()),
        },
        None,
    )?;
    println!("Approve nosman in your browser to continue publishing.");
    println!("Code: {}", start.user_code);
    println!("URL: {}", start.verification_uri_complete);
    println!("Open this page if the browser does not launch: {}", start.verification_uri);
    open_browser(&start.verification_uri_complete);
    let expires_at = DateTime::parse_from_rfc3339(&start.expires_at)
        .map_err(|e| format!("Failed to parse device sign-in expiration: {}", e))?
        .with_timezone(&Utc);

    let interval = start.interval_seconds.max(1);
    loop {
        if Utc::now() >= expires_at {
            return Err("Device sign-in expired. Start again.".to_string());
        }
        thread::sleep(Duration::from_secs(interval));
        let response = client(None)?
            .post(api_url(base_url, "/auth/device/poll"))
            .json(&DevicePollRequest {
                device_code: start.device_code.clone(),
            })
            .send()
            .map_err(|e| format!("Failed to poll device sign-in: {}", e))?;
        if response.status() == reqwest::StatusCode::ACCEPTED {
            continue;
        }
        if !response.status().is_success() {
            return Err(read_error(response));
        }
        let poll = response
            .json::<DevicePollResponse>()
            .map_err(|e| format!("Failed to parse device sign-in response: {}", e))?;
        if let Some(token) = poll.access_token {
            return Ok(token);
        }
    }
}

pub fn ensure_access_token(base_url: &str, interactive: bool) -> Result<String, String> {
    let source = if let Some(token) = env_token() {
        TokenSource::Env(token)
    } else if let Some(token) = load_saved_token() {
        TokenSource::Saved(token)
    } else {
        TokenSource::None
    };

    match source {
        TokenSource::Env(token) => {
            let me = validate_token(base_url, &token)?;
            if me.needs_onboarding {
                return Err("Complete your package-server account profile before publishing.".to_string());
            }
            Ok(token)
        }
        TokenSource::Saved(token) => {
            if let Ok(me) = validate_token(base_url, &token) {
                if me.needs_onboarding {
                    return Err("Complete your package-server account profile before publishing.".to_string());
                }
                return Ok(token);
            }
            if !interactive {
                return Err("Stored package-server token is no longer valid.".to_string());
            }
            let token = start_device_login(base_url)?;
            let me = validate_token(base_url, &token)?;
            if me.needs_onboarding {
                return Err("Complete your package-server account profile before publishing.".to_string());
            }
            save_token(&token)?;
            Ok(token)
        }
        TokenSource::None => {
            if !interactive {
                return Err("A package-server access token is required.".to_string());
            }
            let token = start_device_login(base_url)?;
            let me = validate_token(base_url, &token)?;
            if me.needs_onboarding {
                return Err("Complete your package-server account profile before publishing.".to_string());
            }
            save_token(&token)?;
            Ok(token)
        }
    }
}

pub fn fetch_packages(base_url: &str) -> Result<Vec<PackageSummaryResponse>, String> {
    let mut page = 1;
    let mut packages = Vec::new();
    loop {
        let response: PackageListResponse = get_json(
            base_url,
            &format!("/packages?page={}&per_page=100", page),
            None,
        )?;
        packages.extend(response.packages);
        if !response.has_next_page {
            break;
        }
        page += 1;
    }
    Ok(packages)
}

pub fn fetch_package_detail(base_url: &str, package_name: &str) -> Result<PackageSummaryResponse, String> {
    let response: PackageDetailResponse = get_json(base_url, &format!("/packages/{}", package_name), None)?;
    Ok(response.package)
}

pub fn fetch_package_releases(base_url: &str, package_name: &str) -> Result<Vec<ReleaseSummaryResponse>, String> {
    let mut page = 1;
    let mut releases = Vec::new();
    loop {
        let response: ReleaseListResponse = get_json(
            base_url,
            &format!("/packages/{}/releases?page={}&per_page=100", package_name, page),
            None,
        )?;
        releases.extend(response.releases);
        if !response.has_next_page {
            break;
        }
        page += 1;
    }
    Ok(releases)
}

pub fn to_semver(version: &ApiVersion) -> SemVer {
    SemVer::new(version.major, version.minor, version.patch, None)
}

pub fn to_package_releases(base_url: &str, package_type: &PackageType, releases: Vec<ReleaseSummaryResponse>) -> Vec<PackageReleaseEntry> {
    let mut entries = Vec::new();
    for release in releases {
        for artifact in release.artifacts {
            entries.push(PackageReleaseEntry {
                version: release.version.clone(),
                url: artifact_reference_url(base_url, artifact.id),
                plugin_api_version: if *package_type == PackageType::Plugin {
                    release.api_version.as_ref().map(to_semver)
                } else {
                    None
                },
                subsystem_api_version: if *package_type == PackageType::Subsystem {
                    release.api_version.as_ref().map(to_semver)
                } else {
                    None
                },
                release_date: Some(release.updated_at.clone()),
                dependencies: if release.dependencies.is_empty() {
                    None
                } else {
                    Some(release.dependencies.clone())
                },
                category: None,
                module_tags: None,
                release_tags: if release.tags.is_empty() {
                    None
                } else {
                    Some(release.tags.clone())
                },
                platform: Some(artifact.target_platform),
                node_names: None,
            });
        }
    }
    entries
}

pub fn resolve_download_url(url: &str) -> Result<String, String> {
    if !url.contains("/api/v1/release-artifacts/") || url.contains("/download?token=") {
        return Ok(url.to_string());
    }
    let download_token: DownloadTokenResponse = client(None)?
        .post(format!("{}/download-token", url.trim_end_matches('/')))
        .send()
        .map_err(|e| format!("Failed to request artifact download URL: {}", e))?
        .error_for_status()
        .map_err(|e| format!("Failed to request artifact download URL: {}", e))?
        .json()
        .map_err(|e| format!("Failed to parse artifact download URL: {}", e))?;
    Ok(download_token.download_url)
}

pub fn publish_release(base_url: &str, request: &PublishReleaseRequest, dry_run: bool, verbose: bool) -> Result<ReleaseSummaryResponse, String> {
    if dry_run {
        println!(
            "Would publish {} {} for {} to {}",
            request.name, request.version, request.target_platform, base_url
        );
        return Ok(ReleaseSummaryResponse {
            id: 0,
            version: request.version.clone(),
            tags: request.tags.clone(),
            artifacts: Vec::new(),
            api_version: request.api_version.as_ref().map(|version| ApiVersion {
                major: version.major,
                minor: version.minor,
                patch: version.patch,
            }),
            dependencies: request.dependencies.clone(),
            updated_at: Utc::now().to_rfc3339(),
        });
    }

    let token = ensure_access_token(base_url, true)?;
    let create: PublishSessionResponse = post_json(
        base_url,
        "/publish/sessions",
        &PublishSessionCreateRequest {
            name: request.name.clone(),
            display_name: request.display_name.clone(),
            description: request.description.clone(),
            package_type: request.package_type.clone(),
            category: request.category.clone(),
            version: request.version.clone(),
            api_version: request.api_version.as_ref().map(|version| ApiVersion {
                major: version.major,
                minor: version.minor,
                patch: version.patch,
            }),
            dependencies: request.dependencies.clone(),
            tags: request.tags.clone(),
            artifacts: vec![PublishDraftArtifactRequest {
                target_platform: request.target_platform.clone(),
            }],
        },
        Some(&token),
    )?;

    let upload: PublishUploadUrlResponse = post_json(
        base_url,
        &format!(
            "/publish/sessions/{}/artifact-upload-url",
            create.session.id
        ),
        &PublishArtifactUploadRequest {
            target_platform: request.target_platform.clone(),
        },
        Some(&token),
    )?;

    let bytes = fs::read(&request.artifact_path).map_err(|e| {
        format!(
            "Failed to read artifact {}: {}",
            request.artifact_path.display(),
            e
        )
    })?;
    if verbose {
        println!("Uploading {} bytes to {}", bytes.len(), upload.upload_url);
    }
    let upload_response = client(Some(&token))?
        .put(&upload.upload_url)
        .header(CONTENT_TYPE, "application/octet-stream")
        .body(bytes)
        .send()
        .map_err(|e| format!("Failed to upload artifact: {}", e))?;
    if !upload_response.status().is_success() {
        return Err(read_error(upload_response));
    }

    let finalize_response = post_empty(
        base_url,
        &format!("/publish/sessions/{}/finalize", create.session.id),
        Some(&token),
    )?;
    if !finalize_response.status().is_success() {
        return Err(read_error(finalize_response));
    }
    let finalize = finalize_response
        .json::<PublishFinalizeResponse>()
        .map_err(|e| format!("Failed to parse publish response: {}", e))?;
    Ok(finalize.release)
}

pub fn delete_release(base_url: &str, package_name: &str, version: Option<&String>, dry_run: bool) -> Result<(), String> {
    let token = ensure_access_token(base_url, true)?;
    let mut page = 1;
    let mut releases = Vec::new();
    loop {
        let response: ReleaseListResponse = get_json(
            base_url,
            &format!(
                "/account/packages/{}/releases?page={}&per_page=100",
                package_name, page
            ),
            Some(&token),
        )?;
        releases.extend(response.releases);
        if !response.has_next_page {
            break;
        }
        page += 1;
    }

    let release_ids: Vec<i64> = if let Some(version) = version {
        releases
            .iter()
            .filter(|release| release.version == *version)
            .map(|release| release.id)
            .collect()
    } else {
        releases.iter().map(|release| release.id).collect()
    };
    if release_ids.is_empty() {
        if let Some(version) = version {
            return Err(format!(
                "No release found for package {} version {}",
                package_name, version
            ));
        }
        return Err(format!("No releases found for package {}", package_name));
    }

    if dry_run {
        for release_id in release_ids {
            println!(
                "Would delete package {} release {} from {}",
                package_name, release_id, base_url
            );
        }
        return Ok(());
    }

    for release_id in release_ids {
        delete(
            base_url,
            &format!("/account/packages/{}/releases/{}", package_name, release_id),
            Some(&token),
        )?;
    }
    Ok(())
}
