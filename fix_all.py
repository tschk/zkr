import re

with open('src/store/retrieval.rs', 'r') as f:
    content = f.read()

# For lexical_targets
pattern_feature = r'        let feature_condition = if enabled_features\.is_empty\(\) \{.*?\n        \};\n        let as_of_feature_condition = if enabled_features\.is_empty\(\) \{.*?\n        \};\n'

def replace_lexical(match):
    return '        let features_json = serde_json::to_string(enabled_features).map_err(|e| Error::Invalid(e.to_string()))?;\n'

content = re.sub(pattern_feature, replace_lexical, content, flags=re.DOTALL)

# In lexical_targets None case:
content = content.replace('AND {feature_condition}', "AND (s.feature_flag IS NULL OR (json_array_length(?5) > 0 AND s.feature_flag IN (SELECT value FROM json_each(?5))))")
content = content.replace('vec![&query, &tenant_id.0, &person_id.0, &candidate_limit],', 'vec![&query, &tenant_id.0, &person_id.0, &candidate_limit, &features_json],')

# In lexical_targets Some(as_of) case:
content = content.replace('AND {as_of_feature_condition}', "AND (s.feature_flag IS NULL OR (json_array_length(?7) > 0 AND s.feature_flag IN (SELECT value FROM json_each(?7))))")
content = content.replace('vec![&query, &tenant_id.0, &person_id.0, &as_of.valid_at, &as_of.recorded_at, &candidate_limit],', 'vec![&query, &tenant_id.0, &person_id.0, &as_of.valid_at, &as_of.recorded_at, &candidate_limit, &features_json],')

# Also, remove:
content = re.sub(r'        for feature in enabled_features \{\n            values\.push\(feature\);\n        \}\n', '', content)


# Now for claim_lexical_targets
# In claim_lexical_targets None case:
content = content.replace('vec![&terms_json, &tenant_id.0, &person_id.0, &candidate_limit],', 'vec![&terms_json, &tenant_id.0, &person_id.0, &candidate_limit, &features_json],')
# In claim_lexical_targets Some(as_of) case:
content = content.replace('''                vec![
                    &terms_json,
                    &tenant_id.0,
                    &person_id.0,
                    &as_of.valid_at,
                    &as_of.recorded_at,
                    &candidate_limit,
                ],''', '''                vec![
                    &terms_json,
                    &tenant_id.0,
                    &person_id.0,
                    &as_of.valid_at,
                    &as_of.recorded_at,
                    &candidate_limit,
                    &features_json,
                ],''')

# Replace format!(...) with "..." where format! is no longer needed
content = content.replace('&format!("SELECT s.id, c.id\n                 FROM source_fts', '"SELECT s.id, c.id\n                 FROM source_fts')
content = content.replace('LIMIT ?4"),', 'LIMIT ?4",')
content = content.replace('LIMIT ?6"),', 'LIMIT ?6",')

content = content.replace('format!(\n                    "SELECT id', '"SELECT id')
content = content.replace('LIMIT ?4"\n                ),', 'LIMIT ?4".to_string(),')
content = content.replace('LIMIT ?6"\n                ),', 'LIMIT ?6".to_string(),')


with open('src/store/retrieval.rs', 'w') as f:
    f.write(content)
