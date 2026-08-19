use super::*;

#[test]
fn an_empty_client_list_offers_every_tool() {
    let schemas = schemas_for(&[]);
    assert_eq!(schemas.len(), SUPPORTED.len());
}

#[test]
fn only_the_client_supported_tools_are_offered() {
    let schemas = schemas_for(&[api::ToolType::RunShellCommand as i32]);
    assert_eq!(schemas.len(), 1);
    assert_eq!(schemas[0].name, "run_shell_command");
}
