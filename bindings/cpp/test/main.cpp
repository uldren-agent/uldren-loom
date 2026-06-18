#include <cassert>
#include <algorithm>
#include <chrono>
#include <cstring>
#include <cstdint>
#include <filesystem>
#include <optional>
#include <string>
#include <vector>

#include "loom.hpp"

namespace {

std::string temp_loom_path() {
    auto stamp = std::chrono::steady_clock::now().time_since_epoch().count();
    auto name = "loom-cpp-runtime-" + std::to_string(stamp) + ".loom";
    return (std::filesystem::temp_directory_path() / name).string();
}

std::vector<std::uint8_t> bytes(std::string_view s) {
    return std::vector<std::uint8_t>(s.begin(), s.end());
}

std::vector<std::uint8_t> floats(std::initializer_list<float> values) {
    std::vector<std::uint8_t> out;
    out.reserve(values.size() * sizeof(float));
    for (float value : values) {
        std::uint32_t bits = 0;
        std::memcpy(&bits, &value, sizeof(bits));
        out.push_back(static_cast<std::uint8_t>(bits));
        out.push_back(static_cast<std::uint8_t>(bits >> 8));
        out.push_back(static_cast<std::uint8_t>(bits >> 16));
        out.push_back(static_cast<std::uint8_t>(bits >> 24));
    }
    return out;
}

bool contains(const std::vector<std::uint8_t> &haystack, std::string_view needle) {
    auto n = bytes(needle);
    return std::search(haystack.begin(), haystack.end(), n.begin(), n.end()) != haystack.end();
}

std::string root_id(const std::string &identity_json) {
    const std::string marker = "\"root\":\"";
    auto start = identity_json.find(marker);
    assert(start != std::string::npos);
    start += marker.size();
    auto end = identity_json.find('"', start);
    assert(end != std::string::npos);
    return identity_json.substr(start, end - start);
}

std::string role_id(const std::string &identity_json, std::string_view name) {
    auto name_marker = "\"name\":\"" + std::string(name) + "\"";
    auto name_pos = identity_json.find(name_marker);
    assert(name_pos != std::string::npos);
    const std::string id_marker = "\"id\":\"";
    auto id_pos = identity_json.rfind(id_marker, name_pos);
    assert(id_pos != std::string::npos);
    id_pos += id_marker.size();
    auto end = identity_json.find('"', id_pos);
    assert(end != std::string::npos);
    return identity_json.substr(id_pos, end - id_pos);
}

}  // namespace

int main() {
    assert(!uldren::loom::version().empty());
    assert(uldren::loom::blob_digest(bytes("abc")).rfind("blake3:", 0) == 0);

    auto path = temp_loom_path();
    uldren::loom::create(path, "default");

    {
        auto store = uldren::loom::Loom::open(path);

        auto ns_id = store.workspace_create(std::optional<std::string>{"work"},
                                            std::optional<std::string>{"files"});
        assert(!ns_id.empty());
        assert(store.workspace_list_json().find("\"work\"") != std::string::npos);
        store.workspace_rename("work", "working");
        assert(store.workspace_list_json().find("\"working\"") != std::string::npos);
        store.workspace_delete(ns_id);
        assert(store.workspace_list_json().find("\"working\"") == std::string::npos);
        store.workspace_create(std::optional<std::string>{"policy"},
                               std::optional<std::string>{"vcs"});

        auto identity = store.identity_list_json();
        assert(identity.find("\"authenticated_mode\":false") != std::string::npos);
        auto root = root_id(identity);
        store.identity_set_passphrase(root, "root-pass");
        try {
            (void)store.identity_list_json();
            assert(false);
        } catch (const std::exception &) {
        }
        store.authenticate_passphrase(root, "root-pass");
        auto alice = store.identity_add_principal("alice", "Alice", "user");
        store.identity_set_passphrase(alice, "alice-pass");
        identity = store.identity_list_json();
        assert(identity.find("\"authenticated_mode\":true") != std::string::npos);
        assert(identity.find(alice) != std::string::npos);
        auto reader = role_id(identity, "reader");
        store.identity_assign_role(alice, reader);
        identity = store.identity_list_json();
        assert(identity.find(reader) != std::string::npos);
        assert(store.identity_revoke_role(alice, reader));
        assert(!store.identity_revoke_role(alice, reader));
        store.acl_grant(0, alice, 1, std::nullopt, std::optional<std::string>{"files"});
        auto grants = store.acl_list_json();
        assert(grants.find(alice) != std::string::npos);
        assert(grants.find("\"files\"") != std::string::npos);
        assert(grants.find("\"read\"") != std::string::npos);
        assert(store.acl_revoke(0, alice, 1, std::nullopt, std::optional<std::string>{"files"}));
        assert(!store.acl_revoke(0, alice, 1, std::nullopt, std::optional<std::string>{"files"}));
        store.protected_ref_set("policy", "branch/main", true, false, false, 0, true, false);
        auto protected_policy = store.protected_ref_get_json("policy", "branch/main");
        assert(protected_policy.find("\"fast_forward_only\":true") != std::string::npos);
        assert(protected_policy.find("\"retention_lock\":true") != std::string::npos);
        assert(store.protected_ref_list_json("policy").find("\"ref\":\"branch/main\"") != std::string::npos);
        assert(store.protected_ref_remove("policy", "branch/main"));
        assert(store.protected_ref_get_json("policy", "branch/main") == "null");

        auto watch_db = store.sql_session("watchapp", "main");
        watch_db.exec("CREATE TABLE watch_t (id INTEGER PRIMARY KEY, v TEXT)");
        watch_db.exec("INSERT INTO watch_t VALUES (1, 'a')");
        auto cursor = store.watch_subscribe("watchapp", "main");
        assert(watch_db.commit("seed", "cpp").rfind("blake3:", 0) == 0);
        auto batch = store.watch_poll_bytes(cursor, 10);
        assert(contains(batch, "loom.watch.batch.v1"));
        assert(contains(batch, "unsupported_domains"));
        assert(contains(batch, "sql"));

        auto digest = store.cas_put("blobs", bytes("hello"));
        assert(digest.rfind("blake3:", 0) == 0);
        assert(store.cas_put("blobs", bytes("hello")) == digest);
        assert(store.cas_has("blobs", digest));
        assert(store.cas_get("blobs", digest).value() == bytes("hello"));
        assert(store.cas_list_json("blobs").find(digest) != std::string::npos);
        assert(!store.cas_get("blobs", uldren::loom::blob_digest(bytes("missing"))).has_value());

        assert(store.queue_append("events", "orders", bytes("one")) == 0);
        assert(store.queue_append("events", "orders", bytes("two")) == 1);
        assert(store.queue_len("events", "orders") == 2);
        assert(store.queue_get("events", "orders", 0).value() == bytes("one"));
        assert(!store.queue_get("events", "orders", 9).has_value());
        assert(!store.queue_range_cbor("events", "orders", 0, 2).empty());
        assert(store.queue_consumer_position("events", "orders", "worker") == 0);
        assert(!store.queue_consumer_read_cbor("events", "orders", "worker", 1).empty());
        store.queue_consumer_advance("events", "orders", "worker", 1);
        assert(store.queue_consumer_position("events", "orders", "worker") == 1);
        store.queue_consumer_reset("events", "orders", "worker", 0);
        assert(store.queue_consumer_position("events", "orders", "worker") == 0);

        auto text_put = store.doc_put_text("docs", "notes", "a", "hello text");
        auto text_digest = text_put.digest;
        assert(text_digest.rfind("blake3:", 0) == 0);
        auto text_doc = store.doc_get_text("docs", "notes", "a");
        assert(text_doc.has_value());
        assert(text_doc->text == "hello text");
        assert(text_doc->digest == text_digest);
        assert(!store.doc_get_text("docs", "notes", "missing").has_value());
        try {
            (void)store.doc_put_text("docs", "notes", "a", "stale",
                                     std::optional<std::string>{"entity-tag:00"});
            assert(false);
        } catch (const uldren::loom::error &err) {
            assert(err.code != 0);
        }
        auto updated_digest = store.doc_put_text("docs", "notes", "a", "updated text",
                                                 std::optional<std::string>{text_doc->entity_tag});
        assert(updated_digest.digest != text_digest);
        auto binary_put = store.doc_put_binary("docs", "notes", "raw",
                                               std::vector<std::uint8_t>{0xff, 0x00});
        auto binary_digest = binary_put.digest;
        assert(binary_digest.rfind("blake3:", 0) == 0);
        auto binary_doc = store.doc_get_binary("docs", "notes", "raw");
        assert(binary_doc.has_value());
        assert(binary_doc->bytes == std::vector<std::uint8_t>({0xff, 0x00}));
        assert(binary_doc->digest == binary_digest);
        assert(!store.doc_list_binary("docs", "notes").empty());
        try {
            (void)store.doc_get_text("docs", "notes", "raw");
            assert(false);
        } catch (const uldren::loom::error &err) {
            assert(err.code != 0);
            assert(std::string(err.what()).find("DOCUMENT_NOT_TEXT") != std::string::npos);
        }

        auto point = floats({1.0f, 0.0f});
        auto source = std::string("alpha source");
        store.vector_create("vectors", "emb", 2, 1);
        store.vector_upsert_source("vectors", "emb", "a", point, {}, source,
                                   std::optional<std::string>{"test-embedding"},
                                   std::optional<std::string>{"sha256:test"});
        assert(store.vector_source_text("vectors", "emb", "a").value() == bytes(source));
        assert(contains(store.vector_embedding_model("vectors", "emb").value(), "test-embedding"));
        store.vector_upsert("vectors", "emb", "a", point, {});
        assert(!store.vector_source_text("vectors", "emb", "a").has_value());

        auto db = store.sql_session("app", "main");
        db.exec("CREATE TABLE t (id INTEGER PRIMARY KEY, v TEXT)");
        db.exec("INSERT INTO t VALUES (1, 'a'), (2, 'b')");
        auto result = db.exec("SELECT id, v FROM t ORDER BY id");
        assert(result.is_statements());
        assert(result.column_count(0) == 2);
        assert(result.column_name(0, 0) == "id");
        assert(result.column_name(0, 1) == "v");
        assert(result.row_count(0) == 2);
        assert(result.cell(0, 0, 0).as_int64() == 1);
        assert(result.cell(0, 0, 1).text() == "a");
        assert(result.cell(0, 1, 0).as_int64() == 2);
        assert(result.cell(0, 1, 1).text() == "b");
        assert(db.commit("seed", "cpp").rfind("blake3:", 0) == 0);
    }

    std::filesystem::remove(path);
    return 0;
}
