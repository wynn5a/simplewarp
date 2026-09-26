use super::*;

#[test]
fn test_user_global_skills_defaults_to_empty() {
    assert_eq!(User::test().global_skills, Vec::<String>::new());
}
