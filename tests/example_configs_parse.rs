use genmcp::config::Config;

#[test]
fn podman_example_config_parses() {
    let toml = std::fs::read_to_string("examples/podman_config.toml")
        .expect("failed to read examples/podman_config.toml");
    let config = Config::from_str(&toml).expect("podman example config should parse");

    let group = config
        .groups
        .get("podman")
        .expect("podman group should exist");

    assert!(
        group.tools.iter().any(|t| t.name == "ps"),
        "podman group should include the ps tool"
    );
}

#[test]
fn kubernetes_example_config_parses() {
    let toml = std::fs::read_to_string("examples/kubernetes_config.toml")
        .expect("failed to read examples/kubernetes_config.toml");
    let config = Config::from_str(&toml).expect("kubernetes example config should parse");

    let group = config
        .groups
        .get("kubectl")
        .expect("kubectl group should exist");

    assert!(
        group.tools.iter().any(|t| t.name == "get"),
        "kubectl group should include the get tool"
    );
    assert!(
        group.tools.iter().any(|t| t.name == "apply"),
        "kubectl group should include the apply tool"
    );
}
