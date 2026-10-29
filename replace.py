with open("src/store/lifecycle.rs", "r") as f:
    content = f.read()

bad_fmt = """\
        let (evidence_count, claim_count, changed_claim_ids) =
            execute_deletion_operations(&transaction, &input, &evidence_ids, &candidate_claim_ids)?;
"""

good_fmt = """\
        let (evidence_count, claim_count, changed_claim_ids) = execute_deletion_operations(
            &transaction,
            &input,
            &evidence_ids,
            &candidate_claim_ids,
        )?;
"""

if bad_fmt in content:
    content = content.replace(bad_fmt, good_fmt)
    with open("src/store/lifecycle.rs", "w") as f:
        f.write(content)
