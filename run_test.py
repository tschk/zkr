import re

with open("src/store/tests/apply.rs", "r") as f:
    content = f.read()

test_fn = """
#[test]
fn apply_rejects_empty_scope() {
    let mut replica = empty();

    // Test empty tenant_id
    let mut empty_tenant = apply_input(vec![]);
    empty_tenant.tenant_id = TenantId("".into());
    let error = replica.apply(empty_tenant).unwrap_err();
    assert!(matches!(error, Error::Invalid(msg) if msg.contains("tenant_id must not be empty")));

    // Test empty person_id
    let mut empty_person = apply_input(vec![]);
    empty_person.person_id = PersonId("".into());
    let error = replica.apply(empty_person).unwrap_err();
    assert!(matches!(error, Error::Invalid(msg) if msg.contains("person_id must not be empty")));
}
"""

if "apply_rejects_empty_scope" not in content:
    content += "\n" + test_fn

with open("src/store/tests/apply.rs", "w") as f:
    f.write(content)
