pub fn is_running_in_docker() -> bool {
    std::path::Path::new("/.dockerenv").exists()
}

pub fn get_base_server_url() -> String {
    if is_running_in_docker() {
        "http://host.docker.internal:8080".to_string()
    } else {
        "http://localhost:8080".to_string()
    }
}
