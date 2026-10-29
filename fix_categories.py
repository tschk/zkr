import sys

def run():
    with open('src/store/projects.rs', 'r') as f:
        content = f.read()

    # 1. Update aggregate_dream_categories
    old_agg = "categories.truncate(limit);\n    categories\n}"
    new_agg = "categories\n}"
    if old_agg in content:
        content = content.replace(old_agg, new_agg)
        print("Updated aggregate_dream_categories")
    else:
        print("Failed to find old_agg")

    # 2. Update dream method
    old_dream = """
        let category_suggestions = compute_dream_category_suggestions(
            &claims,
            &claim_vectors,
            &categories,
            min_similarity,
            &active_distance,
            limit,
        )?;

        let schemas = aggregate_dream_schemas(&claims, limit);"""

    new_dream = """
        let category_suggestions = compute_dream_category_suggestions(
            &claims,
            &claim_vectors,
            &categories,
            min_similarity,
            &active_distance,
            limit,
        )?;

        let mut categories = categories;
        categories.truncate(limit);

        let schemas = aggregate_dream_schemas(&claims, limit);"""

    if old_dream in content:
        content = content.replace(old_dream, new_dream)
        print("Updated main dream method")
    else:
        print("Failed to find old_dream")

    # 3. Fix aggregate signature
    old_sig = "fn aggregate_dream_categories(\n    claims: &[LiveDreamClaim],\n    claim_vectors: &HashMap<ClaimId, Vec<f32>>,\n    limit: usize,\n) -> Vec<CategorySummary> {"
    new_sig = "fn aggregate_dream_categories(\n    claims: &[LiveDreamClaim],\n    claim_vectors: &HashMap<ClaimId, Vec<f32>>,\n) -> Vec<CategorySummary> {"

    if old_sig in content:
        content = content.replace(old_sig, new_sig)
        print("Updated signature")
    else:
        print("Failed to find old_sig")

    old_call = "let categories = aggregate_dream_categories(&claims, &claim_vectors, limit);"
    new_call = "let categories = aggregate_dream_categories(&claims, &claim_vectors);"

    if old_call in content:
        content = content.replace(old_call, new_call)
        print("Updated call")
    else:
        print("Failed to find old_call")


    with open('src/store/projects.rs', 'w') as f:
        f.write(content)

run()
