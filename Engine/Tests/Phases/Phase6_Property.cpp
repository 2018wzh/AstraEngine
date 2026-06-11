TEST_CASE("Property system generates schema validates defaults and migrates") {
    Astra::PropertySystem::TypeRegistry registry;
    Astra::PropertySystem::PropertyDescriptor display_name;
    display_name.id = "display_name";
    display_name.type = "string";
    display_name.kind = Astra::PropertySystem::TypeKind::LocalizedText;
    display_name.flags = Astra::PropertySystem::PropertyFlag::RequiresReview;
    display_name.default_value = "Unknown";
    display_name.audit_label = "Display Name";

    Astra::PropertySystem::PropertyDescriptor age;
    age.id = "age";
    age.type = "integer";
    age.default_value = 0;
    age.validation.required = true;
    age.validation.minimum = 0.0;
    age.validation.custom_validator = "positive";

    Astra::PropertySystem::TypeDescriptor character;
    character.type_id = "astra.test.character";
    character.properties = {display_name, age};
    registry.RegisterValidator("positive", [](const nlohmann::json& value) {
        if (value.get<int>() < 0) {
            return Astra::Core::Result<void>::Failure(Astra::Core::ErrorCode::InvalidArgument, "value must be positive");
        }
        return Astra::Core::Result<void>::Success();
    });
    REQUIRE(registry.Register(std::move(character)));

    Astra::Core::DiagnosticSink diagnostics;
    auto schema = registry.GenerateJsonSchema("astra.test.character", diagnostics);
    REQUIRE(schema["properties"].contains("display_name"));
    REQUIRE(schema["required"][0] == "age");

    auto migrated = registry.ApplyMigration({{"name", "Alice"}}, {{"name", "display_name", {}, false}, {"", "age", 1, false}});
    REQUIRE(migrated["display_name"] == "Alice");
    REQUIRE(migrated["age"] == 1);
    auto validated = registry.Validate("astra.test.character", migrated, diagnostics);
    REQUIRE(validated);
    auto diffs = registry.Diff("astra.test.character", {{"display_name", "Alice"}}, {{"display_name", "Bob"}});
    REQUIRE(diffs.size() == 1);
    REQUIRE(diffs[0].requires_review);
}



TEST_CASE("Property system validates nested schema versions and write policy") {
    Astra::PropertySystem::TypeRegistry registry;
    Astra::PropertySystem::TypeDescriptor nested;
    nested.type_id = "astra.test.nested";
    Astra::PropertySystem::PropertyDescriptor nested_value;
    nested_value.id = "value";
    nested_value.type = "string";
    nested_value.kind = Astra::PropertySystem::TypeKind::Scalar;
    nested.properties.push_back(std::move(nested_value));
    REQUIRE(registry.Register(std::move(nested)));

    Astra::PropertySystem::PropertyDescriptor nested_property;
    nested_property.id = "nested";
    nested_property.type = "astra.test.nested";
    nested_property.kind = Astra::PropertySystem::TypeKind::Struct;

    Astra::PropertySystem::PropertyDescriptor array_property;
    array_property.id = "nested_array";
    array_property.type = "astra.test.nested";
    array_property.kind = Astra::PropertySystem::TypeKind::Array;

    Astra::PropertySystem::PropertyDescriptor map_property;
    map_property.id = "nested_map";
    map_property.type = "astra.test.nested";
    map_property.kind = Astra::PropertySystem::TypeKind::Map;

    Astra::PropertySystem::PropertyDescriptor union_property;
    union_property.id = "choice";
    union_property.type = "astra.test.nested";
    union_property.kind = Astra::PropertySystem::TypeKind::TaggedUnion;

    Astra::PropertySystem::PropertyDescriptor editable;
    editable.id = "display_name";
    editable.type = "string";
    editable.flags = Astra::PropertySystem::PropertyFlag::AiEditable | Astra::PropertySystem::PropertyFlag::RequiresReview;

    Astra::PropertySystem::PropertyDescriptor guarded;
    guarded.id = "package_hash";
    guarded.type = "string";
    guarded.flags = Astra::PropertySystem::PropertyFlag::ReadOnly | Astra::PropertySystem::PropertyFlag::ReleaseSensitive;

    Astra::PropertySystem::PropertyDescriptor editor_only;
    editor_only.id = "editor_note";
    editor_only.type = "string";
    editor_only.flags = Astra::PropertySystem::PropertyFlag::EditorOnly;

    Astra::PropertySystem::PropertyDescriptor runtime_only;
    runtime_only.id = "runtime_counter";
    runtime_only.type = "integer";
    runtime_only.flags = Astra::PropertySystem::PropertyFlag::RuntimeOnly;

    Astra::PropertySystem::TypeDescriptor root;
    root.type_id = "astra.test.root";
    root.properties = {nested_property, array_property, map_property, union_property, editable, guarded, editor_only, runtime_only};
    REQUIRE(registry.Register(std::move(root)));
    REQUIRE(registry.RegisterMigration({"astra.test.root", 1, 2, {}}));

    Astra::Core::DiagnosticSink diagnostics;
    auto schema = registry.GenerateJsonSchema("astra.test.root", diagnostics);
    REQUIRE(schema["properties"]["nested"]["properties"].contains("value"));
    REQUIRE(schema["properties"]["nested_array"]["type"] == "array");
    REQUIRE(schema["properties"]["nested_array"]["items"]["properties"].contains("value"));
    REQUIRE(schema["properties"]["nested_map"]["additionalProperties"]["properties"].contains("value"));
    REQUIRE(schema["properties"]["choice"]["properties"].contains("value"));
    REQUIRE(registry.ValidateSchemaVersion("astra.test.root", 1, 2, diagnostics));

    auto allowed = registry.EvaluateWrite({"astra.test.root", "display_name", "Alice", "Bob", true, false, false, false});
    REQUIRE(allowed.allowed);
    REQUIRE(allowed.requires_review);
    auto ai_denied = registry.EvaluateWrite({"astra.test.root", "package_hash", "old", "new", true, false, false, false});
    REQUIRE_FALSE(ai_denied.allowed);
    auto editor_denied = registry.EvaluateWrite({"astra.test.root", "runtime_counter", 1, 2, false, true, false, false});
    REQUIRE_FALSE(editor_denied.allowed);
    auto runtime_denied = registry.EvaluateWrite({"astra.test.root", "editor_note", "old", "new", false, false, true, false});
    REQUIRE_FALSE(runtime_denied.allowed);

    auto denied = registry.EvaluateWrite({"astra.test.root", "package_hash", "old", "new", false, true, false, true});
    REQUIRE_FALSE(denied.allowed);
    REQUIRE(denied.release_sensitive);
    REQUIRE(Astra::PropertySystem::ToJson(denied)["diagnostics"].size() >= 1);

    auto diffs = registry.Diff("astra.test.root", {{"package_hash", "old"}}, {{"package_hash", "new"}});
    REQUIRE(diffs[0].release_sensitive);
    REQUIRE(Astra::PropertySystem::ToJson(diffs[0])["release_sensitive"] == true);
}



