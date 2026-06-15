use command_mcp::config::Config;

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

#[test]
fn unixtools_example_config_parses() {
    let toml = std::fs::read_to_string("examples/unixtools_config.toml")
        .expect("failed to read examples/unixtools_config.toml");
    let config = Config::from_str(&toml).expect("unixtools example config should parse");

    let file_ops_group = config
        .groups
        .get("file_operations")
        .expect("file_operations group should exist");

    assert!(
        file_ops_group.tools.iter().any(|t| t.name == "mkdir"),
        "file_operations group should include the mkdir tool"
    );
    assert!(
        file_ops_group.tools.iter().any(|t| t.name == "ls"),
        "file_operations group should include the ls tool"
    );
    assert!(
        file_ops_group.tools.iter().any(|t| t.name == "touch"),
        "file_operations group should include the touch tool"
    );
    assert!(
        file_ops_group.tools.iter().any(|t| t.name == "pwd"),
        "file_operations group should include the pwd tool"
    );
    assert!(
        file_ops_group.tools.iter().any(|t| t.name == "bash"),
        "file_operations group should include the bash tool"
    );
    assert!(
        file_ops_group.tools.iter().any(|t| t.name == "write_file"),
        "file_operations group should include the write_file tool"
    );

    let text_group = config
        .groups
        .get("text_processing")
        .expect("text_processing group should exist");

    assert!(
        text_group.tools.iter().any(|t| t.name == "cat"),
        "text_processing group should include the cat tool"
    );
    assert!(
        text_group.tools.iter().any(|t| t.name == "echo"),
        "text_processing group should include the echo tool"
    );
    assert!(
        text_group.tools.iter().any(|t| t.name == "diff"),
        "text_processing group should include the diff tool"
    );
    assert!(
        text_group.tools.iter().any(|t| t.name == "patch"),
        "text_processing group should include the patch tool"
    );
}
