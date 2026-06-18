#pragma once
#include <utility>

#include "row_stream.hpp"

namespace uldren::loom {

class batch;
class sql;

struct acl_scope {
    std::int32_t kind;
    std::vector<std::uint8_t> prefix;

    static acl_scope ref(std::vector<std::uint8_t> prefix) { return {0, std::move(prefix)}; }
    static acl_scope collection(std::vector<std::uint8_t> prefix) { return {1, std::move(prefix)}; }
    static acl_scope path(std::vector<std::uint8_t> prefix) { return {2, std::move(prefix)}; }
    static acl_scope key(std::vector<std::uint8_t> prefix) { return {3, std::move(prefix)}; }
    static acl_scope table(std::vector<std::uint8_t> prefix) { return {4, std::move(prefix)}; }
    static acl_scope exec(std::vector<std::uint8_t> prefix) { return {5, std::move(prefix)}; }
};

struct document_text {
    std::string text;
    std::string digest;
    std::string entity_tag;
};

struct document_binary {
    std::vector<std::uint8_t> bytes;
    std::string digest;
    std::string entity_tag;
};

struct document_put_result {
    std::string digest;
    std::string entity_tag;
};

/// A generic open `.loom` session for workspace lifecycle operations.
class Loom {
public:
    static Loom open(const std::string &path) {
        LoomSession *handle = nullptr;
        detail::check(::loom_open(path.c_str(), &handle));
        return Loom(handle, path, "", {});
    }

    static Loom open_keyed(const std::string &path, const std::string &passphrase) {
        LoomSession *handle = nullptr;
        detail::check(::loom_open_keyed(
            path.c_str(), reinterpret_cast<const unsigned char *>(passphrase.data()),
            passphrase.size(), &handle));
        return Loom(handle, path, passphrase, {});
    }

    static Loom open_with_kek(const std::string &path, const std::vector<std::uint8_t> &kek) {
        LoomSession *handle = nullptr;
        detail::check(::loom_open_with_kek(path.c_str(), kek.data(), kek.size(), &handle));
        return Loom(handle, path, "", kek);
    }

    ~Loom() { ::loom_close(handle_); }

    Loom(const Loom &) = delete;
    Loom &operator=(const Loom &) = delete;
    Loom(Loom &&other) noexcept : handle_(other.handle_) { other.handle_ = nullptr; }
    Loom &operator=(Loom &&other) noexcept {
        if (this != &other) {
            ::loom_close(handle_);
            handle_ = other.handle_;
            other.handle_ = nullptr;
        }
        return *this;
    }

    std::string workspace_create(const std::optional<std::string> &name = std::nullopt,
                                 const std::optional<std::string> &facet = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_workspace_create(handle_, name ? name->c_str() : nullptr,
                                              facet ? facet->c_str() : nullptr, &out));
        return detail::take_string(out);
    }

    std::string workspace_list_json() const {
        char *out = nullptr;
        detail::check(::loom_workspace_list_json(handle_, &out));
        return detail::take_string(out);
    }

    void workspace_rename(const std::string &ns, const std::string &new_name) {
        detail::check(::loom_workspace_rename(handle_, ns.c_str(), new_name.c_str()));
    }

    void workspace_delete(const std::string &ns) {
        detail::check(::loom_workspace_delete(handle_, ns.c_str()));
    }

    /// Authenticate `principal` with a passphrase and bind the resulting identity to this session.
    void authenticate_passphrase(const std::string &principal, const std::string &passphrase) {
        detail::check(::loom_authenticate_passphrase(
            handle_, principal.c_str(), reinterpret_cast<const unsigned char *>(passphrase.data()),
            passphrase.size()));
        auth_principal_ = principal;
        auth_passphrase_ = passphrase;
    }

    /// Clear the authenticated identity bound to this session.
    void clear_authentication() {
        detail::check(::loom_clear_authentication(handle_));
        auth_principal_.clear();
        auth_passphrase_.clear();
    }

    sql sql_session(const std::string &ns, const std::string &db) const;

    batch sql_batch(const std::string &ns, const std::string &db) const;

    /// Execute a canonical `loom.exec.request.v1` request; returns `loom.exec.result.v1` bytes.
    std::vector<std::uint8_t> exec_cbor(const std::vector<std::uint8_t> &request) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_exec_cbor(handle_, request.data(), request.size(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> lanes_create(const std::string &workspace,
                                           const std::vector<std::uint8_t> &lane) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_lanes_create_cbor(handle_, workspace.c_str(), lane.data(), lane.size(),
                                               &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::optional<std::vector<std::uint8_t>> lanes_get(const std::string &workspace,
                                                       const std::string &lane_id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_lanes_get_cbor(handle_, workspace.c_str(), lane_id.c_str(), &ptr, &len,
                                            &found));
        if (!found) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> lanes_list(const std::string &workspace) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_lanes_list_cbor(handle_, workspace.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> lanes_update(
        const std::string &workspace, const std::string &lane_id,
        const std::optional<std::string> &title, const std::optional<std::string> &description,
        const std::optional<std::string> &lane_status,
        const std::optional<std::string> &status_report,
        const std::optional<std::string> &reviewer_feedback, const std::string &updated_by) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_lanes_update_cbor(
            handle_, workspace.c_str(), lane_id.c_str(), title ? title->c_str() : nullptr,
            description ? description->c_str() : nullptr,
            lane_status ? lane_status->c_str() : nullptr,
            status_report ? status_report->c_str() : nullptr,
            reviewer_feedback ? reviewer_feedback->c_str() : nullptr, updated_by.c_str(), &ptr,
            &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> lanes_ticket_add(
        const std::string &workspace, const std::string &lane_id, const std::string &ticket_id,
        const std::string &updated_by, const std::string &placement = "append",
        const std::optional<std::string> &anchor = std::nullopt) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_lanes_ticket_add_cbor(handle_, workspace.c_str(), lane_id.c_str(),
                                                   ticket_id.c_str(), updated_by.c_str(),
                                                   placement.c_str(),
                                                   anchor ? anchor->c_str() : nullptr, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> lanes_ticket_remove(const std::string &workspace,
                                                  const std::string &lane_id,
                                                  const std::string &ticket_id,
                                                  const std::string &updated_by) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_lanes_ticket_remove_cbor(handle_, workspace.c_str(), lane_id.c_str(),
                                                      ticket_id.c_str(), updated_by.c_str(), &ptr,
                                                      &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::string spaces_create_json(const std::string &workspace,
                                   const std::string &page_workspace_id,
                                   const std::string &space_id,
                                   const std::string &title,
                                   const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_spaces_create_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                                space_id.c_str(), title.c_str(),
                                                expected_root ? expected_root->c_str() : nullptr,
                                                &out));
        return detail::take_string(out);
    }

    std::string spaces_list_json(const std::string &workspace,
                                 const std::string &page_workspace_id) {
        char *out = nullptr;
        detail::check(::loom_spaces_list_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                              &out));
        return detail::take_string(out);
    }

    std::string spaces_get_json(const std::string &workspace,
                                const std::string &page_workspace_id,
                                const std::string &space_id) {
        char *out = nullptr;
        detail::check(::loom_spaces_get_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                             space_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string pages_create_json(const std::string &workspace,
                                  const std::string &page_workspace_id,
                                  const std::string &page_id,
                                  const std::string &space_id,
                                  const std::optional<std::string> &parent_page_id,
                                  const std::string &title,
                                  const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_pages_create_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), page_id.c_str(), space_id.c_str(),
            parent_page_id ? parent_page_id->c_str() : nullptr, title.c_str(),
            expected_root ? expected_root->c_str() : nullptr, &out));
        return detail::take_string(out);
    }

    std::string pages_update_json(const std::string &workspace,
                                  const std::string &page_workspace_id,
                                  const std::string &page_id,
                                  const std::string &body_text,
                                  const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_pages_update_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), page_id.c_str(),
            body_text.c_str(), expected_root ? expected_root->c_str() : nullptr, &out));
        return detail::take_string(out);
    }

    std::string pages_publish_json(const std::string &workspace,
                                   const std::string &page_workspace_id,
                                   const std::string &page_id,
                                   const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_pages_publish_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), page_id.c_str(),
            expected_root ? expected_root->c_str() : nullptr, &out));
        return detail::take_string(out);
    }

    std::string pages_get_json(const std::string &workspace,
                               const std::string &page_workspace_id,
                               const std::string &page_id) {
        char *out = nullptr;
        detail::check(::loom_pages_get_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                            page_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string pages_list_json(const std::string &workspace,
                                const std::string &page_workspace_id) {
        char *out = nullptr;
        detail::check(::loom_pages_list_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                             &out));
        return detail::take_string(out);
    }

    std::string pages_history_json(const std::string &workspace,
                                   const std::string &page_workspace_id,
                                   const std::string &page_id) {
        char *out = nullptr;
        detail::check(::loom_pages_history_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                                page_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string structures_create_json(
        const std::string &workspace, const std::string &page_workspace_id,
        const std::string &structure_id, const std::string &space_id, const std::string &kind,
        const std::string &title, const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_structures_create_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), structure_id.c_str(),
            space_id.c_str(), kind.c_str(), title.c_str(),
            expected_root ? expected_root->c_str() : nullptr, &out));
        return detail::take_string(out);
    }

    std::string structures_add_node_json(
        const std::string &workspace, const std::string &page_workspace_id,
        const std::string &structure_id, const std::string &node_id, const std::string &kind,
        const std::string &label, const std::optional<std::string> &body_digest = std::nullopt,
        const std::optional<std::string> &entity_ref = std::nullopt,
        const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_structures_add_node_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), structure_id.c_str(),
            node_id.c_str(), kind.c_str(), label.c_str(), body_digest ? body_digest->c_str() : nullptr,
            entity_ref ? entity_ref->c_str() : nullptr, expected_root ? expected_root->c_str() : nullptr,
            &out));
        return detail::take_string(out);
    }

    std::string structures_update_node_json(
        const std::string &workspace, const std::string &page_workspace_id,
        const std::string &structure_id, const std::string &node_id, const std::string &kind,
        const std::string &label, const std::optional<std::string> &body_digest = std::nullopt,
        const std::optional<std::string> &entity_ref = std::nullopt,
        const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_structures_update_node_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), structure_id.c_str(),
            node_id.c_str(), kind.c_str(), label.c_str(), body_digest ? body_digest->c_str() : nullptr,
            entity_ref ? entity_ref->c_str() : nullptr, expected_root ? expected_root->c_str() : nullptr,
            &out));
        return detail::take_string(out);
    }

    std::string structures_bind_json(
        const std::string &workspace, const std::string &page_workspace_id,
        const std::string &structure_id, const std::string &node_id,
        const std::optional<std::string> &entity_ref = std::nullopt,
        const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_structures_bind_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), structure_id.c_str(),
            node_id.c_str(), entity_ref ? entity_ref->c_str() : nullptr,
            expected_root ? expected_root->c_str() : nullptr, &out));
        return detail::take_string(out);
    }

    std::string structures_move_node_json(
        const std::string &workspace, const std::string &page_workspace_id,
        const std::string &structure_id, const std::string &node_id,
        const std::optional<std::string> &parent_node_id = std::nullopt,
        const std::optional<std::string> &label = std::nullopt,
        const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_structures_move_node_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), structure_id.c_str(),
            node_id.c_str(), parent_node_id ? parent_node_id->c_str() : nullptr,
            label ? label->c_str() : nullptr, expected_root ? expected_root->c_str() : nullptr,
            &out));
        return detail::take_string(out);
    }

    std::string structures_link_node_json(
        const std::string &workspace, const std::string &page_workspace_id,
        const std::string &structure_id, const std::string &edge_id,
        const std::string &src_node_id, const std::string &dst_node_id, const std::string &label,
        const std::optional<std::string> &target_ref = std::nullopt,
        const std::optional<std::string> &expected_root = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_structures_link_node_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), structure_id.c_str(),
            edge_id.c_str(), src_node_id.c_str(), dst_node_id.c_str(), label.c_str(),
            target_ref ? target_ref->c_str() : nullptr, expected_root ? expected_root->c_str() : nullptr,
            &out));
        return detail::take_string(out);
    }

    std::string structures_decompose_to_tickets_json(
        const std::string &workspace, const std::string &page_workspace_id,
        const std::string &structure_id, const std::string &items_json) {
        char *out = nullptr;
        detail::check(::loom_structures_decompose_to_tickets_json(
            handle_, workspace.c_str(), page_workspace_id.c_str(), structure_id.c_str(),
            items_json.c_str(), &out));
        return detail::take_string(out);
    }

    std::string structures_get_json(const std::string &workspace,
                                    const std::string &page_workspace_id,
                                    const std::string &structure_id) {
        char *out = nullptr;
        detail::check(::loom_structures_get_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                                 structure_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string structures_list_json(const std::string &workspace,
                                     const std::string &page_workspace_id) {
        char *out = nullptr;
        detail::check(::loom_structures_list_json(handle_, workspace.c_str(), page_workspace_id.c_str(),
                                                  &out));
        return detail::take_string(out);
    }

    void metrics_put_descriptor(const std::string &workspace,
                                const std::vector<std::uint8_t> &descriptor) {
        detail::check(::loom_metrics_put_descriptor(handle_, workspace.c_str(), descriptor.data(),
                                                    descriptor.size()));
    }

    std::optional<std::vector<std::uint8_t>> metrics_get_descriptor(const std::string &workspace,
                                                                    const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t present = 0;
        detail::check(
            ::loom_metrics_get_descriptor(handle_, workspace.c_str(), name.c_str(), &ptr, &len, &present));
        if (present == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    void metrics_put_observation(const std::string &workspace,
                                 const std::string &descriptor_name,
                                 const std::vector<std::uint8_t> &observation) {
        detail::check(::loom_metrics_put_observation(handle_, workspace.c_str(), descriptor_name.c_str(),
                                                     observation.data(), observation.size()));
    }

    std::vector<std::uint8_t> metrics_query(const std::string &workspace,
                                            const std::string &descriptor_name,
                                            std::uint64_t from_timestamp_ms,
                                            std::uint64_t to_timestamp_ms,
                                            std::uint32_t max_series,
                                            std::uint32_t max_groups,
                                            std::uint32_t max_samples,
                                            std::uint64_t max_output_bytes,
                                            std::uint64_t now_timestamp_ms) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_metrics_query_cbor(
            handle_, workspace.c_str(), descriptor_name.c_str(), from_timestamp_ms, to_timestamp_ms,
            max_series, max_groups, max_samples, max_output_bytes, now_timestamp_ms, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::string logs_put_record(const std::string &workspace,
                                const std::vector<std::uint8_t> &record) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_logs_put_record(handle_, workspace.c_str(), record.data(), record.size(), &ptr, &len));
        auto bytes = detail::take_result_bytes(ptr, len);
        return std::string(bytes.begin(), bytes.end());
    }

    std::optional<std::vector<std::uint8_t>> logs_get_record(const std::string &workspace,
                                                             const std::string &record_id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t present = 0;
        detail::check(
            ::loom_logs_get_record(handle_, workspace.c_str(), record_id.c_str(), &ptr, &len, &present));
        if (present == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> logs_query(const std::string &workspace,
                                         std::uint64_t from_time_unix_nano,
                                         std::uint64_t to_time_unix_nano,
                                         std::uint32_t max_records,
                                         std::uint64_t max_output_bytes) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_logs_query_cbor(handle_, workspace.c_str(), from_time_unix_nano,
                                             to_time_unix_nano, max_records, max_output_bytes, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    void traces_put_span(const std::string &workspace,
                         const std::vector<std::uint8_t> &span) {
        detail::check(
            ::loom_traces_put_span(handle_, workspace.c_str(), span.data(), span.size()));
    }

    std::optional<std::vector<std::uint8_t>> traces_get_span(const std::string &workspace,
                                                             const std::string &trace_id,
                                                             const std::string &span_id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t present = 0;
        detail::check(::loom_traces_get_span(handle_, workspace.c_str(), trace_id.c_str(),
                                             span_id.c_str(), &ptr, &len, &present));
        if (present == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> traces_trace_spans(const std::string &workspace,
                                                 const std::string &trace_id,
                                                 std::uint32_t max_spans,
                                                 std::uint64_t max_output_bytes) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_traces_trace_spans_cbor(handle_, workspace.c_str(), trace_id.c_str(),
                                                     max_spans, max_output_bytes, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> traces_query(const std::string &workspace,
                                           std::uint64_t from_start_time_ns,
                                           std::uint64_t to_start_time_ns,
                                           std::uint32_t max_spans,
                                           std::uint64_t max_output_bytes) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_traces_query_cbor(handle_, workspace.c_str(), from_start_time_ns,
                                               to_start_time_ns, max_spans, max_output_bytes, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> fs_import(const std::string &ns, const std::string &src_path,
                                        bool commit = false, bool dry_run = false) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_fs_import(handle_, ns.c_str(), src_path.c_str(), commit ? 1 : 0,
                                       dry_run ? 1 : 0, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> fs_export(const std::string &ns, const std::string &dst_path,
                                        const std::optional<std::string> &revision = std::nullopt,
                                        bool dry_run = false) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_fs_export(handle_, ns.c_str(), dst_path.c_str(),
                                       revision ? revision->c_str() : nullptr,
                                       dry_run ? 1 : 0, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> archive_import(const std::string &ns, const std::string &src_path,
                                             const std::string &kind, bool dry_run = false) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_archive_import(handle_, ns.c_str(), src_path.c_str(), kind.c_str(),
                                            dry_run ? 1 : 0, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> archive_export(const std::string &ns, const std::string &dst_path,
                                             const std::string &kind,
                                             const std::optional<std::string> &revision = std::nullopt,
                                             bool dry_run = false) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_archive_export(handle_, ns.c_str(), dst_path.c_str(), kind.c_str(),
                                            revision ? revision->c_str() : nullptr,
                                            dry_run ? 1 : 0, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> car_import(const std::string &src_path, bool dry_run = false) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_car_import(handle_, src_path.c_str(), dry_run ? 1 : 0, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::vector<std::uint8_t> car_export(const std::string &ns, const std::string &dst_path,
                                         bool dry_run = false) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_car_export(handle_, ns.c_str(), dst_path.c_str(), dry_run ? 1 : 0,
                                        &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::string identity_list_json() const {
        char *out = nullptr;
        detail::check(::loom_identity_list_json(handle_, &out));
        return detail::take_string(out);
    }

    std::string identity_add_principal(const std::string &principal_handle, const std::string &name,
                                       const std::string &kind = "user") {
        char *out = nullptr;
        detail::check(::loom_identity_add_principal(
            handle_, principal_handle.c_str(), name.c_str(), kind.c_str(), &out));
        return detail::take_string(out);
    }

    void identity_rename_principal_handle(const std::string &principal,
                                          const std::string &principal_handle) {
        detail::check(::loom_identity_rename_principal_handle(
            handle_, principal.c_str(), principal_handle.c_str()));
    }

    void identity_set_passphrase(const std::string &principal,
                                 const std::string &passphrase) {
        detail::check(::loom_identity_set_passphrase(
            handle_, principal.c_str(), reinterpret_cast<const unsigned char *>(passphrase.data()),
            passphrase.size()));
    }

    void identity_remove_principal(const std::string &principal) {
        detail::check(::loom_identity_remove_principal(handle_, principal.c_str()));
    }

    void identity_assign_role(const std::string &principal, const std::string &role) {
        detail::check(::loom_identity_assign_role(handle_, principal.c_str(), role.c_str()));
    }

    bool identity_revoke_role(const std::string &principal, const std::string &role) {
        std::int32_t removed = 0;
        detail::check(::loom_identity_revoke_role(handle_, principal.c_str(), role.c_str(), &removed));
        return removed != 0;
    }

    std::string identity_create_external_credential(const std::string &principal,
                                                    const std::string &kind,
                                                    const std::string &label,
                                                    const std::string &issuer,
                                                    const std::string &subject,
                                                    const std::string &material_digest = "") {
        char *out = nullptr;
        const char *digest = material_digest.empty() ? nullptr : material_digest.c_str();
        detail::check(::loom_identity_create_external_credential(
            handle_, principal.c_str(), kind.c_str(), label.c_str(), issuer.c_str(),
            subject.c_str(), digest, &out));
        return detail::take_string(out);
    }

    void identity_revoke_external_credential(const std::string &credential) {
        detail::check(::loom_identity_revoke_external_credential(handle_, credential.c_str()));
    }

    std::string identity_add_public_key(const std::string &principal,
                                        const std::string &label,
                                        const std::string &algorithm,
                                        const std::string &public_key_hex) {
        char *out = nullptr;
        detail::check(::loom_identity_add_public_key(
            handle_, principal.c_str(), label.c_str(), algorithm.c_str(), public_key_hex.c_str(),
            &out));
        return detail::take_string(out);
    }

    void identity_revoke_public_key(const std::string &key) {
        detail::check(::loom_identity_revoke_public_key(handle_, key.c_str()));
    }

    std::string acl_list_json() const {
        char *out = nullptr;
        detail::check(::loom_acl_list_json(handle_, &out));
        return detail::take_string(out);
    }

    void acl_grant(std::int32_t effect, const std::string &subject, std::uint32_t rights_mask,
                   const std::optional<std::string> &ns = std::nullopt,
                   const std::optional<std::string> &facet = std::nullopt) {
        detail::check(::loom_acl_grant(handle_, effect, subject.c_str(),
                                       ns ? ns->c_str() : nullptr,
                                       facet ? facet->c_str() : nullptr, rights_mask));
    }

    bool acl_revoke(std::int32_t effect, const std::string &subject, std::uint32_t rights_mask,
                    const std::optional<std::string> &ns = std::nullopt,
                    const std::optional<std::string> &facet = std::nullopt) {
        std::int32_t removed = 0;
        detail::check(::loom_acl_revoke(handle_, effect, subject.c_str(),
                                        ns ? ns->c_str() : nullptr,
                                        facet ? facet->c_str() : nullptr, rights_mask, &removed));
        return removed != 0;
    }

    void acl_grant_scoped(std::int32_t effect, const std::string &subject,
                          std::uint32_t rights_mask,
                          const std::optional<std::string> &ns,
                          const std::optional<std::string> &facet,
                          const std::optional<std::string> &ref_glob,
                          const std::vector<acl_scope> &scopes) {
        auto packed = pack_acl_scopes(scopes);
        detail::check(::loom_acl_grant_scoped(
            handle_, effect, subject.c_str(), ns ? ns->c_str() : nullptr,
            facet ? facet->c_str() : nullptr, rights_mask, ref_glob ? ref_glob->c_str() : nullptr,
            scopes.size(), packed.kinds.data(), packed.prefixes.data(), packed.lens.data()));
    }

    void acl_grant_scoped_predicate(std::int32_t effect, const std::string &subject,
                                    std::uint32_t rights_mask,
                                    const std::optional<std::string> &ns,
                                    const std::optional<std::string> &facet,
                                    const std::optional<std::string> &ref_glob,
                                    const std::vector<acl_scope> &scopes,
                                    const std::optional<std::string> &predicate_cel) {
        auto packed = pack_acl_scopes(scopes);
        detail::check(::loom_acl_grant_scoped_predicate(
            handle_, effect, subject.c_str(), ns ? ns->c_str() : nullptr,
            facet ? facet->c_str() : nullptr, rights_mask, ref_glob ? ref_glob->c_str() : nullptr,
            scopes.size(), packed.kinds.data(), packed.prefixes.data(), packed.lens.data(),
            predicate_cel ? "cel" : nullptr,
            predicate_cel ? predicate_cel->c_str() : nullptr));
    }

    bool acl_revoke_scoped(std::int32_t effect, const std::string &subject,
                           std::uint32_t rights_mask,
                           const std::optional<std::string> &ns,
                           const std::optional<std::string> &facet,
                           const std::optional<std::string> &ref_glob,
                           const std::vector<acl_scope> &scopes) {
        auto packed = pack_acl_scopes(scopes);
        std::int32_t removed = 0;
        detail::check(::loom_acl_revoke_scoped(
            handle_, effect, subject.c_str(), ns ? ns->c_str() : nullptr,
            facet ? facet->c_str() : nullptr, rights_mask, ref_glob ? ref_glob->c_str() : nullptr,
            scopes.size(), packed.kinds.data(), packed.prefixes.data(), packed.lens.data(),
            &removed));
        return removed != 0;
    }

    bool acl_revoke_scoped_predicate(std::int32_t effect, const std::string &subject,
                                     std::uint32_t rights_mask,
                                     const std::optional<std::string> &ns,
                                     const std::optional<std::string> &facet,
                                     const std::optional<std::string> &ref_glob,
                                     const std::vector<acl_scope> &scopes,
                                     const std::optional<std::string> &predicate_cel) {
        auto packed = pack_acl_scopes(scopes);
        std::int32_t removed = 0;
        detail::check(::loom_acl_revoke_scoped_predicate(
            handle_, effect, subject.c_str(), ns ? ns->c_str() : nullptr,
            facet ? facet->c_str() : nullptr, rights_mask, ref_glob ? ref_glob->c_str() : nullptr,
            scopes.size(), packed.kinds.data(), packed.prefixes.data(), packed.lens.data(),
            predicate_cel ? "cel" : nullptr,
            predicate_cel ? predicate_cel->c_str() : nullptr, &removed));
        return removed != 0;
    }

    std::string protected_ref_list_json(const std::string &ns) const {
        char *out = nullptr;
        detail::check(::loom_protected_ref_list_json(handle_, ns.c_str(), &out));
        return detail::take_string(out);
    }

    std::string protected_ref_get_json(const std::string &ns, const std::string &ref_name) const {
        char *out = nullptr;
        detail::check(::loom_protected_ref_get_json(handle_, ns.c_str(), ref_name.c_str(), &out));
        return detail::take_string(out);
    }

    void protected_ref_set(const std::string &ns, const std::string &ref_name,
                           bool fast_forward_only, bool signed_commits_required,
                           bool signed_ref_advance_required, std::uint32_t required_review_count,
                           bool retention_lock, bool governance_lock) {
        detail::check(::loom_protected_ref_set(handle_, ns.c_str(), ref_name.c_str(),
                                               fast_forward_only, signed_commits_required,
                                               signed_ref_advance_required, required_review_count,
                                               retention_lock, governance_lock));
    }

    bool protected_ref_remove(const std::string &ns, const std::string &ref_name) {
        std::int32_t removed = 0;
        detail::check(::loom_protected_ref_remove(handle_, ns.c_str(), ref_name.c_str(), &removed));
        return removed != 0;
    }

    /// Add a passphrase unlock wrap to this encrypted store (opened with an unlocking credential).
    /// `allow_no_recovery` permits leaving no passphrase recovery wrap. The host supplies the secret.
    void key_add_wrap_keyed(const std::string &passphrase, bool allow_no_recovery = false) {
        detail::check(::loom_key_add_wrap_keyed(
            handle_, reinterpret_cast<const unsigned char *>(passphrase.data()), passphrase.size(),
            allow_no_recovery));
    }

    /// Add a host-supplied 256-bit raw-KEK unlock wrap to this encrypted store. `kek` must be 32 bytes;
    /// the host acquires it from a keychain, Secure Enclave, passkey PRF, or KMS.
    void key_add_wrap_with_kek(const std::vector<std::uint8_t> &kek, bool allow_no_recovery = false) {
        detail::check(
            ::loom_key_add_wrap_with_kek(handle_, kek.data(), kek.size(), allow_no_recovery));
    }

    /// Remove one unlock wrap by zero-based `index`. `allow_no_recovery` permits removing the last
    /// passphrase recovery wrap.
    void key_remove_wrap(std::uintptr_t index, bool allow_no_recovery = false) {
        detail::check(::loom_key_remove_wrap(handle_, index, allow_no_recovery));
    }

    /// Store `content` in the `cas` facet of workspace `ns` (by UUID or name, created if absent); returns
    /// the content address (`"algo:hex"`). Idempotent: identical bytes yield the same address.
    std::string cas_put(const std::string &ns, const std::vector<std::uint8_t> &content) {
        char *out = nullptr;
        detail::check(::loom_cas_put(handle_, ns.c_str(), content.data(), content.size(), &out));
        return detail::take_string(out);
    }

    /// Fetch the blob addressed by `digest` from workspace `ns`, or `std::nullopt` if absent. An invalid
    /// digest throws `INVALID_ARGUMENT`; a content/digest mismatch throws `INTEGRITY_FAILURE`.
    std::optional<std::vector<std::uint8_t>> cas_get(const std::string &ns,
                                                     const std::string &digest) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_cas_get(handle_, ns.c_str(), digest.c_str(), &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Whether a blob addressed by `digest` is present in workspace `ns`. An invalid digest throws
    /// `INVALID_ARGUMENT`.
    bool cas_has(const std::string &ns, const std::string &digest) {
        std::int32_t found = 0;
        detail::check(::loom_cas_has(handle_, ns.c_str(), digest.c_str(), &found));
        return found != 0;
    }

    /// Drop the blob addressed by `digest` from workspace `ns`'s working tree (unreachable going
    /// forward); returns whether it was present. CAS stays immutable: bytes are GC-reclaimed once
    /// unreferenced, and an earlier commit that held the blob still restores it.
    bool cas_delete(const std::string &ns, const std::string &digest) {
        std::int32_t found = 0;
        detail::check(::loom_cas_delete(handle_, ns.c_str(), digest.c_str(), &found));
        return found != 0;
    }

    /// List the content addresses in workspace `ns`'s `cas` facet as a sorted JSON string array.
    std::string cas_list_json(const std::string &ns) {
        char *out = nullptr;
        detail::check(::loom_cas_list_json(handle_, ns.c_str(), &out));
        return detail::take_string(out);
    }

    std::string meetings_import_snapshot(const std::string &ns, const std::string &input_profile,
                                         const std::vector<std::uint8_t> &snapshot,
                                         bool dry_run = false) {
        char *out = nullptr;
        detail::check(::loom_meetings_import_snapshot(handle_, ns.c_str(), input_profile.c_str(),
                                                      snapshot.data(), snapshot.size(),
                                                      dry_run ? 1 : 0, &out));
        return detail::take_string(out);
    }

    std::vector<std::uint8_t> meetings_source_read(const std::string &ns,
                                                   const std::string &source_id,
                                                   const std::string &leaf) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_meetings_source_read(handle_, ns.c_str(), source_id.c_str(),
                                                  leaf.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::string drive_list_json(const std::string &ns, const std::string &workspace_id,
                                const std::string &folder_id) {
        char *out = nullptr;
        detail::check(::loom_drive_list_json(handle_, ns.c_str(), workspace_id.c_str(),
                                             folder_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_stat_json(const std::string &ns, const std::string &workspace_id,
                                const std::string &folder_id, const std::string &name) {
        char *out = nullptr;
        detail::check(::loom_drive_stat_json(handle_, ns.c_str(), workspace_id.c_str(),
                                             folder_id.c_str(), name.c_str(), &out));
        return detail::take_string(out);
    }

    std::vector<std::uint8_t> drive_read_file(const std::string &ns,
                                              const std::string &workspace_id,
                                              const std::string &file_id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_drive_read(handle_, ns.c_str(), workspace_id.c_str(),
                                        file_id.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    std::string drive_list_versions_json(const std::string &ns,
                                         const std::string &workspace_id,
                                         const std::string &file_id) {
        char *out = nullptr;
        detail::check(::loom_drive_list_versions_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                      file_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_list_conflicts_json(const std::string &ns,
                                          const std::string &workspace_id) {
        char *out = nullptr;
        detail::check(
            ::loom_drive_list_conflicts_json(handle_, ns.c_str(), workspace_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_list_shares_json(const std::string &ns,
                                       const std::string &workspace_id) {
        char *out = nullptr;
        detail::check(
            ::loom_drive_list_shares_json(handle_, ns.c_str(), workspace_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_list_retention_json(const std::string &ns,
                                          const std::string &workspace_id) {
        char *out = nullptr;
        detail::check(
            ::loom_drive_list_retention_json(handle_, ns.c_str(), workspace_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_create_folder_json(const std::string &ns,
                                         const std::string &workspace_id,
                                         const std::string &parent_folder_id,
                                         const std::string &folder_id, const std::string &name,
                                         const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_drive_create_folder_json(
            handle_, ns.c_str(), workspace_id.c_str(), parent_folder_id.c_str(),
            folder_id.c_str(), name.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_create_upload_json(const std::string &ns,
                                         const std::string &workspace_id,
                                         const std::string &upload_id,
                                         const std::string &parent_folder_id,
                                         const std::string &name, const std::string &file_id,
                                         const std::string &expected_root,
                                         std::uint64_t created_at_ms, bool replace_file) {
        char *out = nullptr;
        detail::check(::loom_drive_create_upload_json(
            handle_, ns.c_str(), workspace_id.c_str(), upload_id.c_str(),
            parent_folder_id.c_str(), name.c_str(), file_id.c_str(), expected_root.c_str(),
            created_at_ms, replace_file ? 1 : 0, &out));
        return detail::take_string(out);
    }

    std::string drive_upload_chunk_json(const std::string &ns,
                                        const std::string &workspace_id,
                                        const std::string &upload_id,
                                        const std::vector<std::uint8_t> &chunk) {
        char *out = nullptr;
        detail::check(::loom_drive_upload_chunk_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                     upload_id.c_str(), chunk.data(),
                                                     chunk.size(), &out));
        return detail::take_string(out);
    }

    std::string drive_commit_upload_json(const std::string &ns,
                                         const std::string &workspace_id,
                                         const std::string &upload_id) {
        char *out = nullptr;
        detail::check(::loom_drive_commit_upload_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                      upload_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_rename_json(const std::string &ns, const std::string &workspace_id,
                                  const std::string &folder_id, const std::string &node_id,
                                  const std::string &new_name,
                                  const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_drive_rename_json(handle_, ns.c_str(), workspace_id.c_str(),
                                               folder_id.c_str(), node_id.c_str(),
                                               new_name.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_move_json(const std::string &ns, const std::string &workspace_id,
                                const std::string &source_folder_id,
                                const std::string &target_folder_id,
                                const std::string &node_id, const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_drive_move_json(
            handle_, ns.c_str(), workspace_id.c_str(), source_folder_id.c_str(),
            target_folder_id.c_str(), node_id.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_delete_json(const std::string &ns, const std::string &workspace_id,
                                  const std::string &folder_id, const std::string &node_id,
                                  const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_drive_delete_json(handle_, ns.c_str(), workspace_id.c_str(),
                                               folder_id.c_str(), node_id.c_str(),
                                               expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_resolve_conflict_json(const std::string &ns,
                                            const std::string &workspace_id,
                                            const std::string &conflict_id,
                                            const std::string &resolution) {
        char *out = nullptr;
        detail::check(::loom_drive_resolve_conflict_json(
            handle_, ns.c_str(), workspace_id.c_str(), conflict_id.c_str(), resolution.c_str(),
            &out));
        return detail::take_string(out);
    }

    std::string drive_grant_share_json(const std::string &ns, const std::string &workspace_id,
                                       const std::string &grant_id,
                                       const std::string &target_kind,
                                       const std::string &target_id,
                                       const std::string &principal, const std::string &role,
                                       std::uint64_t granted_at_ms,
                                       std::optional<std::uint64_t> expires_at_ms = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_drive_grant_share_json(
            handle_, ns.c_str(), workspace_id.c_str(), grant_id.c_str(), target_kind.c_str(),
            target_id.c_str(), principal.c_str(), role.c_str(), granted_at_ms,
            expires_at_ms.value_or(0), expires_at_ms ? 1 : 0, &out));
        return detail::take_string(out);
    }

    std::string drive_revoke_share_json(const std::string &ns,
                                        const std::string &workspace_id,
                                        const std::string &grant_id) {
        char *out = nullptr;
        detail::check(::loom_drive_revoke_share_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                     grant_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string drive_apply_share_expiry_json(const std::string &ns,
                                              const std::string &workspace_id,
                                              std::uint64_t now_ms) {
        char *out = nullptr;
        detail::check(::loom_drive_apply_share_expiry_json(handle_, ns.c_str(),
                                                           workspace_id.c_str(), now_ms, &out));
        return detail::take_string(out);
    }

    std::string drive_pin_retention_json(
        const std::string &ns, const std::string &workspace_id, const std::string &pin_id,
        const std::string &kind, const std::string &root,
        const std::optional<std::string> &target_entity_id, std::uint64_t added_at_ms,
        std::optional<std::uint64_t> expires_at_ms = std::nullopt) {
        char *out = nullptr;
        detail::check(::loom_drive_pin_retention_json(
            handle_, ns.c_str(), workspace_id.c_str(), pin_id.c_str(), kind.c_str(),
            root.c_str(), target_entity_id ? target_entity_id->c_str() : nullptr,
            added_at_ms, expires_at_ms.value_or(0), expires_at_ms ? 1 : 0, &out));
        return detail::take_string(out);
    }

    std::string drive_unpin_retention_json(const std::string &ns,
                                           const std::string &workspace_id,
                                           const std::string &pin_id) {
        char *out = nullptr;
        detail::check(::loom_drive_unpin_retention_json(handle_, ns.c_str(),
                                                        workspace_id.c_str(), pin_id.c_str(),
                                                        &out));
        return detail::take_string(out);
    }

    std::string drive_apply_retention_json(const std::string &ns,
                                           const std::string &workspace_id,
                                           std::uint64_t now_ms) {
        char *out = nullptr;
        detail::check(::loom_drive_apply_retention_json(handle_, ns.c_str(),
                                                        workspace_id.c_str(), now_ms, &out));
        return detail::take_string(out);
    }

    std::string tickets_project_create_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &project_id, const std::string &key_prefix,
        const std::string &name, const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_project_create_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(), key_prefix.c_str(),
            name.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_project_rekey_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &project_id, const std::string &key_prefix,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_project_rekey_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(), key_prefix.c_str(),
            expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_project_settings_get_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &project_id) {
        char *out = nullptr;
        detail::check(::loom_tickets_project_settings_get_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_project_settings_set_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &project_id, const std::string &default_projection,
        const std::string &enable_projections_json,
        const std::string &disable_projections_json,
        const std::string &actor_enforcement,
        const std::string &project_owner_principal,
        bool clear_project_owner_principal,
        const std::string &acceptance_authorities_json,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_project_settings_set_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(),
            default_projection.c_str(), enable_projections_json.c_str(),
            disable_projections_json.c_str(), actor_enforcement.c_str(),
            project_owner_principal.c_str(), clear_project_owner_principal,
            acceptance_authorities_json.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_fields_json(const std::string &ns, const std::string &workspace_id,
                                    const std::string &project_id,
                                    const std::string &projection,
                                    const std::string &operation) {
        char *out = nullptr;
        detail::check(::loom_tickets_fields_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(), projection.c_str(),
            operation.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_field_put_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &project_id, const std::string &field_id,
        const std::string &key, const std::string &name,
        const std::string &description, const std::string &field_type,
        const std::string &option_set, uint32_t max_length, bool has_max_length,
        bool required, bool searchable, bool orderable,
        const std::string &cardinality, const std::string &applicable_type_ids_json,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_field_put_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(), field_id.c_str(),
            key.c_str(), name.c_str(), description.c_str(), field_type.c_str(),
            option_set.c_str(), max_length, has_max_length, required, searchable, orderable,
            cardinality.c_str(), applicable_type_ids_json.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_field_retire_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &project_id, const std::string &field_id,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_field_retire_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(), field_id.c_str(),
            expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_create_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &project_id, const std::string &ticket_type,
        const std::string &external_source, const std::string &external_id,
        const std::string &fields_json, const std::string &policy_labels_json,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_create_json(
            handle_, ns.c_str(), workspace_id.c_str(), project_id.c_str(), ticket_type.c_str(),
            external_source.c_str(), external_id.c_str(), fields_json.c_str(),
            policy_labels_json.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_update_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &set_fields_json,
        const std::string &delete_fields_json, const std::string &action,
        const std::string &target_status, const std::string &observed_source_status,
        const std::string &observed_workflow_version, const std::string &assignee,
        const std::string &expected_root) {
        return tickets_update_json(ns, workspace_id, ticket_id, set_fields_json,
                                   delete_fields_json, action, target_status,
                                   observed_source_status, observed_workflow_version, assignee, "",
                                   "", "", expected_root, "", "", "");
    }

    std::string tickets_update_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &set_fields_json,
        const std::string &delete_fields_json, const std::string &action,
        const std::string &target_status, const std::string &observed_source_status,
        const std::string &observed_workflow_version, const std::string &assignee,
        const std::string &comment_id, const std::string &comment_type,
        const std::string &comment_body, const std::string &expected_root) {
        return tickets_update_json(ns, workspace_id, ticket_id, set_fields_json,
                                   delete_fields_json, action, target_status,
                                   observed_source_status, observed_workflow_version, assignee,
                                   comment_id, comment_type, comment_body, expected_root, "", "",
                                   "");
    }

    std::string tickets_update_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &set_fields_json,
        const std::string &delete_fields_json, const std::string &action,
        const std::string &target_status, const std::string &observed_source_status,
        const std::string &observed_workflow_version, const std::string &assignee,
        const std::string &comment_id, const std::string &comment_type,
        const std::string &comment_body, const std::string &expected_root,
        const std::string &comments_json, const std::string &relation_sets_json,
        const std::string &relation_removes_json) {
        char *out = nullptr;
        detail::check(::loom_tickets_update_json(
            handle_, ns.c_str(), workspace_id.c_str(), ticket_id.c_str(), set_fields_json.c_str(),
            delete_fields_json.c_str(), action.c_str(), target_status.c_str(),
            observed_source_status.c_str(), observed_workflow_version.c_str(), assignee.c_str(),
            comment_id.c_str(), comment_type.c_str(), comment_body.c_str(), expected_root.c_str(),
            comments_json.c_str(), relation_sets_json.c_str(), relation_removes_json.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_delete_json(const std::string &ns, const std::string &workspace_id,
                                    const std::string &ticket_id,
                                    const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_delete_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                 ticket_id.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_comments_json(const std::string &ns, const std::string &workspace_id,
                                      const std::string &ticket_id) {
        char *out = nullptr;
        detail::check(::loom_tickets_comments_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                   ticket_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_comment_add_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &comment_id,
        const std::string &comment_type, const std::string &body,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_comment_add_json(
            handle_, ns.c_str(), workspace_id.c_str(), ticket_id.c_str(), comment_id.c_str(),
            comment_type.c_str(), body.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_comment_update_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &comment_id,
        const std::string &comment_type, const std::string &body,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_comment_update_json(
            handle_, ns.c_str(), workspace_id.c_str(), ticket_id.c_str(), comment_id.c_str(),
            comment_type.c_str(), body.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_comment_delete_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &comment_id,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_comment_delete_json(
            handle_, ns.c_str(), workspace_id.c_str(), ticket_id.c_str(), comment_id.c_str(),
            expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_relation_set_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &relation_id,
        const std::string &kind, const std::string &target_id,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_relation_set_json(
            handle_, ns.c_str(), workspace_id.c_str(), ticket_id.c_str(), relation_id.c_str(),
            kind.c_str(), target_id.c_str(), expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_relation_remove_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &ticket_id, const std::string &relation_id,
        const std::string &expected_root) {
        char *out = nullptr;
        detail::check(::loom_tickets_relation_remove_json(
            handle_, ns.c_str(), workspace_id.c_str(), ticket_id.c_str(), relation_id.c_str(),
            expected_root.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_get_json(const std::string &ns, const std::string &workspace_id,
                                 const std::string &ticket_id,
                                 const std::string &projection) {
        char *out = nullptr;
        detail::check(::loom_tickets_get_json(handle_, ns.c_str(), workspace_id.c_str(),
                                              ticket_id.c_str(), projection.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_list_json(const std::string &ns, const std::string &workspace_id,
                                  const std::string &projection) {
        char *out = nullptr;
        detail::check(::loom_tickets_list_json(handle_, ns.c_str(), workspace_id.c_str(),
                                               projection.c_str(), &out));
        return detail::take_string(out);
    }

    std::string tickets_history_json(const std::string &ns, const std::string &workspace_id,
                                     const std::string &ticket_id) {
        char *out = nullptr;
        detail::check(::loom_tickets_history_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                  ticket_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_create_channel_json(const std::string &ns, const std::string &workspace_id,
                                         const std::string &channel_id,
                                         const std::string &channel_handle,
                                         const std::string &name) {
        char *out = nullptr;
        detail::check(::loom_chat_create_channel_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                      channel_id.c_str(), channel_handle.c_str(),
                                                      name.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_rename_channel_json(const std::string &ns, const std::string &workspace_id,
                                         const std::string &selector,
                                         const std::string &channel_handle) {
        char *out = nullptr;
        detail::check(::loom_chat_rename_channel_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                      selector.c_str(), channel_handle.c_str(),
                                                      &out));
        return detail::take_string(out);
    }

    std::string chat_list_channels_json(const std::string &ns,
                                        const std::string &workspace_id) {
        char *out = nullptr;
        detail::check(::loom_chat_list_channels_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                     &out));
        return detail::take_string(out);
    }

    std::string chat_post_message_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &channel_id, const std::string &message_id,
        const std::string &thread_id, const std::string &body_text) {
        char *out = nullptr;
        detail::check(::loom_chat_post_message_json(
            handle_, ns.c_str(), workspace_id.c_str(), channel_id.c_str(), message_id.c_str(),
            thread_id.c_str(), body_text.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_edit_message_json(const std::string &ns,
                                       const std::string &workspace_id,
                                       const std::string &channel_id,
                                       const std::string &message_id,
                                       const std::string &body_text) {
        char *out = nullptr;
        detail::check(::loom_chat_edit_message_json(
            handle_, ns.c_str(), workspace_id.c_str(), channel_id.c_str(), message_id.c_str(),
            body_text.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_redact_message_json(const std::string &ns,
                                         const std::string &workspace_id,
                                         const std::string &channel_id,
                                         const std::string &message_id,
                                         const std::string &reason) {
        char *out = nullptr;
        detail::check(::loom_chat_redact_message_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                      channel_id.c_str(), message_id.c_str(),
                                                      reason.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_create_thread_json(const std::string &ns,
                                        const std::string &workspace_id,
                                        const std::string &channel_id,
                                        const std::string &thread_id,
                                        const std::string &parent_message_id) {
        char *out = nullptr;
        detail::check(::loom_chat_create_thread_json(
            handle_, ns.c_str(), workspace_id.c_str(), channel_id.c_str(), thread_id.c_str(),
            parent_message_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_create_task_json(const std::string &ns, const std::string &workspace_id,
                                      const std::string &channel_id,
                                      const std::string &task_id,
                                      const std::string &message_id,
                                      const std::string &title) {
        char *out = nullptr;
        detail::check(::loom_chat_create_task_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                   channel_id.c_str(), task_id.c_str(),
                                                   message_id.c_str(), title.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_claim_task_json(const std::string &ns,
                                     const std::string &workspace_id,
                                     const std::string &channel_id,
                                     const std::string &task_id,
                                     const std::string &claim_id,
                                     const std::string &lease_token) {
        char *out = nullptr;
        detail::check(::loom_chat_claim_task_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                  channel_id.c_str(), task_id.c_str(),
                                                  claim_id.c_str(), lease_token.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_complete_task_json(const std::string &ns,
                                        const std::string &workspace_id,
                                        const std::string &channel_id,
                                        const std::string &task_id,
                                        const std::string &claim_id,
                                        const std::string &result_message_id) {
        char *out = nullptr;
        detail::check(::loom_chat_complete_task_json(
            handle_, ns.c_str(), workspace_id.c_str(), channel_id.c_str(), task_id.c_str(),
            claim_id.c_str(), result_message_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_invoke_agent_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &channel_id, const std::string &invocation_id,
        const std::string &agent_principal, const std::string &source_message_ids_json,
        const std::string &prompt_text) {
        char *out = nullptr;
        detail::check(::loom_chat_invoke_agent_json(
            handle_, ns.c_str(), workspace_id.c_str(), channel_id.c_str(), invocation_id.c_str(),
            agent_principal.c_str(), source_message_ids_json.c_str(), prompt_text.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_agent_reply_json(const std::string &ns,
                                      const std::string &workspace_id,
                                      const std::string &channel_id,
                                      const std::string &invocation_id,
                                      const std::string &message_id) {
        char *out = nullptr;
        detail::check(::loom_chat_agent_reply_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                   channel_id.c_str(), invocation_id.c_str(),
                                                   message_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_request_handoff_json(
        const std::string &ns, const std::string &workspace_id,
        const std::string &channel_id, const std::string &handoff_id,
        const std::string &from_agent_principal, const std::string &to_principal,
        const std::string &reason) {
        char *out = nullptr;
        detail::check(::loom_chat_request_handoff_json(
            handle_, ns.c_str(), workspace_id.c_str(), channel_id.c_str(), handoff_id.c_str(),
            from_agent_principal.c_str(), to_principal.c_str(), reason.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_add_reaction_json(const std::string &ns,
                                       const std::string &workspace_id,
                                       const std::string &channel_id,
                                       const std::string &message_id,
                                       const std::string &kind) {
        char *out = nullptr;
        detail::check(::loom_chat_add_reaction_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                    channel_id.c_str(), message_id.c_str(),
                                                    kind.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_remove_reaction_json(const std::string &ns,
                                          const std::string &workspace_id,
                                          const std::string &channel_id,
                                          const std::string &message_id,
                                          const std::string &kind) {
        char *out = nullptr;
        detail::check(::loom_chat_remove_reaction_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                       channel_id.c_str(), message_id.c_str(),
                                                       kind.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_emoji_list_json(const std::string &ns,
                                     const std::string &workspace_id) {
        char *out = nullptr;
        detail::check(::loom_chat_emoji_list_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                  &out));
        return detail::take_string(out);
    }

    std::string chat_emoji_register_json(const std::string &ns,
                                         const std::string &workspace_id,
                                         const std::string &kind) {
        char *out = nullptr;
        detail::check(::loom_chat_emoji_register_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                      kind.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_emoji_unregister_json(const std::string &ns,
                                           const std::string &workspace_id,
                                           const std::string &kind) {
        char *out = nullptr;
        detail::check(::loom_chat_emoji_unregister_json(handle_, ns.c_str(),
                                                        workspace_id.c_str(), kind.c_str(),
                                                        &out));
        return detail::take_string(out);
    }

    std::string chat_messages_json(const std::string &ns, const std::string &workspace_id,
                                   const std::string &channel_id) {
        char *out = nullptr;
        detail::check(::loom_chat_messages_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                channel_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_cursor_json(const std::string &ns, const std::string &workspace_id,
                                 const std::string &channel_id) {
        char *out = nullptr;
        detail::check(::loom_chat_cursor_json(handle_, ns.c_str(), workspace_id.c_str(),
                                              channel_id.c_str(), &out));
        return detail::take_string(out);
    }

    std::string chat_update_cursor_json(const std::string &ns,
                                        const std::string &workspace_id,
                                        const std::string &channel_id,
                                        std::uint64_t next_sequence) {
        char *out = nullptr;
        detail::check(::loom_chat_update_cursor_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                     channel_id.c_str(), next_sequence, &out));
        return detail::take_string(out);
    }

    std::string chat_fetch_events_json(const std::string &ns,
                                       const std::string &workspace_id,
                                       const std::string &channel_id,
                                       std::uint64_t from_sequence, std::uintptr_t max) {
        char *out = nullptr;
        detail::check(::loom_chat_fetch_events_json(handle_, ns.c_str(), workspace_id.c_str(),
                                                    channel_id.c_str(), from_sequence, max, &out));
        return detail::take_string(out);
    }

    /// Put `value` at the typed `key` (Loom Canonical CBOR cell) in map `collection` of workspace `ns` (by
    /// UUID or name, created with the `kv` facet if absent). A later put at the same key replaces it.
    void kv_put(const std::string &ns, const std::string &collection,
                const std::vector<std::uint8_t> &key, const std::vector<std::uint8_t> &value) {
        detail::check(::loom_kv_put(handle_, ns.c_str(), collection.c_str(), key.data(), key.size(),
                                    value.data(), value.size()));
    }

    /// Fetch the value at typed `key` in map `collection` of workspace `ns`, or `std::nullopt` if the key or
    /// map is absent.
    std::optional<std::vector<std::uint8_t>> kv_get(const std::string &ns, const std::string &collection,
                                                    const std::vector<std::uint8_t> &key) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_kv_get(handle_, ns.c_str(), collection.c_str(), key.data(), key.size(), &ptr,
                                    &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Remove the typed `key` from map `collection` of workspace `ns`; returns whether it was present.
    bool kv_delete(const std::string &ns, const std::string &collection,
                   const std::vector<std::uint8_t> &key) {
        std::int32_t found = 0;
        detail::check(
            ::loom_kv_delete(handle_, ns.c_str(), collection.c_str(), key.data(), key.size(), &found));
        return found != 0;
    }

    /// List map `collection` of workspace `ns` as the Loom Canonical CBOR array of `[key, value]` pairs in
    /// key order (an absent map is the empty array).
    std::vector<std::uint8_t> kv_list(const std::string &ns, const std::string &collection) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_kv_list_cbor(handle_, ns.c_str(), collection.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The entries of map `collection` with `lo <= key < hi` (half-open, key order) as the Loom Canonical
    /// CBOR array of `[key, value]` pairs. `lo`/`hi` are typed-cell CBOR keys.
    std::vector<std::uint8_t> kv_range(const std::string &ns, const std::string &collection,
                                       const std::vector<std::uint8_t> &lo,
                                       const std::vector<std::uint8_t> &hi) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_kv_range_cbor(handle_, ns.c_str(), collection.c_str(), lo.data(), lo.size(),
                                           hi.data(), hi.size(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    // Tags: eviction 0 none/1 lru/2 lfu/3 random/4 fifo/5 ttl_priority; on_evict 0 drop/1 write_through;
    // back_pressure 0 block/1 pressure/2 assisted. max_entries/max_bytes/flush_batch 0 = unbounded;
    // flush_high_water_pct < 0 = only the hard bound.
    void management_kv_set_config(const std::string &ns, const std::string &collection, std::int32_t tier,
                                  std::uint64_t default_ttl_ms = 0,
                                  std::uint64_t default_idle_ttl_ms = 0, bool read_through = false,
                                  bool write_through = false, std::uint64_t max_entries = 0,
                                  std::uint64_t max_bytes = 0, std::int32_t eviction = 0,
                                  std::int32_t on_evict = 0, bool write_behind = false,
                                  bool write_around = false, std::int32_t back_pressure = 0,
                                  std::int32_t flush_high_water_pct = -1,
                                  std::uint64_t flush_batch = 0) {
        detail::check(::loom_management_kv_set_config(
            handle_, ns.c_str(), collection.c_str(), tier, default_ttl_ms, default_idle_ttl_ms,
            read_through, write_through, max_entries, max_bytes, eviction, on_evict, write_behind,
            write_around, back_pressure, flush_high_water_pct, flush_batch));
    }

    std::string management_kv_get_config_json(const std::string &ns, const std::string &collection) {
        char *out = nullptr;
        detail::check(::loom_management_kv_get_config_json(handle_, ns.c_str(), collection.c_str(), &out));
        return detail::take_string(out);
    }

    /// Put UTF-8 text at `id` in collection `collection` and return the new document tags.
    document_put_result doc_put_text(const std::string &ns, const std::string &collection,
                                     const std::string &id, const std::string &text,
                                     const std::optional<std::string> &expected_entity_tag = std::nullopt) {
        char *digest = nullptr;
        char *entity_tag = nullptr;
        detail::check(::loom_doc_put_text(handle_, ns.c_str(), collection.c_str(), id.c_str(),
                                          text.c_str(),
                                          expected_entity_tag ? expected_entity_tag->c_str() : nullptr,
                                          &digest, &entity_tag));
        return document_put_result{detail::take_string(digest), detail::take_string(entity_tag)};
    }

    /// Fetch `id` as UTF-8 text with its content digest, or `std::nullopt` if absent.
    std::optional<document_text> doc_get_text(const std::string &ns,
                                              const std::string &collection,
                                              const std::string &id) {
        char *text = nullptr;
        char *digest = nullptr;
        char *entity_tag = nullptr;
        std::int32_t found = 0;
        detail::check(::loom_doc_get_text(handle_, ns.c_str(), collection.c_str(), id.c_str(),
                                          &text, &digest, &entity_tag, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return document_text{detail::take_string(text), detail::take_string(digest),
                             detail::take_string(entity_tag)};
    }

    /// Put binary bytes at `id` in collection `collection` and return the new document tags.
    document_put_result doc_put_binary(const std::string &ns, const std::string &collection,
                                       const std::string &id,
                                       const std::vector<std::uint8_t> &bytes,
                                       const std::optional<std::string> &expected_entity_tag = std::nullopt) {
        char *digest = nullptr;
        char *entity_tag = nullptr;
        detail::check(::loom_doc_put_binary(
            handle_, ns.c_str(), collection.c_str(), id.c_str(), bytes.data(), bytes.size(),
            expected_entity_tag ? expected_entity_tag->c_str() : nullptr, &digest, &entity_tag));
        return document_put_result{detail::take_string(digest), detail::take_string(entity_tag)};
    }

    /// Fetch `id` as binary bytes with its content digest, or `std::nullopt` if absent.
    std::optional<document_binary> doc_get_binary(const std::string &ns,
                                                  const std::string &collection,
                                                  const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        char *digest = nullptr;
        char *entity_tag = nullptr;
        std::int32_t found = 0;
        detail::check(::loom_doc_get_binary(handle_, ns.c_str(), collection.c_str(), id.c_str(),
                                            &ptr, &len, &digest, &entity_tag, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return document_binary{detail::take_result_bytes(ptr, len), detail::take_string(digest),
                               detail::take_string(entity_tag)};
    }

    /// Remove `id` from collection `collection`; returns whether it was present.
    bool doc_delete(const std::string &ns, const std::string &collection, const std::string &id) {
        std::int32_t found = 0;
        detail::check(::loom_doc_delete(handle_, ns.c_str(), collection.c_str(), id.c_str(), &found));
        return found != 0;
    }

    /// List collection `collection` as its canonical binary representation.
    std::vector<std::uint8_t> doc_list_binary(const std::string &ns,
                                              const std::string &collection) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_doc_list_binary_cbor(handle_, ns.c_str(), collection.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    void doc_index_create(const std::string &ns, const std::string &collection,
                          const std::string &name, const std::string &path, bool unique = false) {
        detail::check(::loom_doc_index_create(handle_, ns.c_str(), collection.c_str(), name.c_str(),
                                              path.c_str(), unique ? 1 : 0));
    }

    void doc_index_create_json(const std::string &ns, const std::string &collection,
                               const std::string &declaration_json) {
        detail::check(::loom_doc_index_create_json(
            handle_, ns.c_str(), collection.c_str(),
            reinterpret_cast<const uint8_t *>(declaration_json.data()), declaration_json.size()));
    }

    bool doc_index_drop(const std::string &ns, const std::string &collection, const std::string &name) {
        std::int32_t found = 0;
        detail::check(::loom_doc_index_drop(handle_, ns.c_str(), collection.c_str(), name.c_str(),
                                            &found));
        return found != 0;
    }

    void doc_index_rebuild(const std::string &ns, const std::string &collection,
                           const std::string &name) {
        detail::check(::loom_doc_index_rebuild(handle_, ns.c_str(), collection.c_str(), name.c_str()));
    }

    std::string doc_index_list_json(const std::string &ns, const std::string &collection) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_doc_index_list_json(handle_, ns.c_str(), collection.c_str(), &ptr, &len));
        auto bytes = detail::take_result_bytes(ptr, len);
        return std::string(bytes.begin(), bytes.end());
    }

    std::string doc_index_status_json(const std::string &ns, const std::string &collection) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_doc_index_status_json(handle_, ns.c_str(), collection.c_str(), &ptr, &len));
        auto bytes = detail::take_result_bytes(ptr, len);
        return std::string(bytes.begin(), bytes.end());
    }

    std::string doc_find_json(const std::string &ns, const std::string &collection,
                              const std::string &index, const std::string &value_json) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_doc_find_json(
            handle_, ns.c_str(), collection.c_str(), index.c_str(),
            reinterpret_cast<const unsigned char *>(value_json.data()), value_json.size(), &ptr, &len));
        auto bytes = detail::take_result_bytes(ptr, len);
        return std::string(bytes.begin(), bytes.end());
    }

    std::string doc_query_json(const std::string &ns, const std::string &collection,
                               const std::string &query_json) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_doc_query_json(
            handle_, ns.c_str(), collection.c_str(),
            reinterpret_cast<const unsigned char *>(query_json.data()), query_json.size(), &ptr, &len));
        auto bytes = detail::take_result_bytes(ptr, len);
        return std::string(bytes.begin(), bytes.end());
    }

    /// Record `value` at timestamp `ts` in series `collection` of workspace `ns` (created with the
    /// `time-series` facet if absent). A repeated timestamp replaces the point.
    void ts_put(const std::string &ns, const std::string &collection, std::int64_t ts,
                const std::vector<std::uint8_t> &value) {
        detail::check(::loom_ts_put(handle_, ns.c_str(), collection.c_str(), ts, value.data(),
                                    value.size()));
    }

    /// Fetch the point at `ts` in series `collection`, or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> ts_get(const std::string &ns, const std::string &collection,
                                                    std::int64_t ts) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_ts_get(handle_, ns.c_str(), collection.c_str(), ts, &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// The points of series `collection` with `from <= ts < to` (half-open, time order) as the Loom Canonical
    /// CBOR array of `[ts, value]` pairs.
    std::vector<std::uint8_t> ts_range(const std::string &ns, const std::string &collection,
                                       std::int64_t from, std::int64_t to) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_ts_range_cbor(handle_, ns.c_str(), collection.c_str(), from, to, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The most recent point of series `collection` as `(ts, value)`, or `std::nullopt` if absent/empty.
    std::optional<std::pair<std::int64_t, std::vector<std::uint8_t>>>
    ts_latest(const std::string &ns, const std::string &collection) {
        std::int64_t ts = 0;
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_ts_latest(handle_, ns.c_str(), collection.c_str(), &ts, &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return std::make_pair(ts, detail::take_result_bytes(ptr, len));
    }

    /// Append `payload` to ledger `collection` of workspace `ns` (created with the `ledger` facet if absent);
    /// returns the new entry's zero-based sequence.
    std::uint64_t ledger_append(const std::string &ns, const std::string &collection,
                                const std::vector<std::uint8_t> &payload) {
        std::uint64_t seq = 0;
        detail::check(::loom_ledger_append(handle_, ns.c_str(), collection.c_str(), payload.data(),
                                           payload.size(), &seq));
        return seq;
    }

    /// Fetch the payload at `seq` in ledger `collection`, or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> ledger_get(const std::string &ns,
                                                        const std::string &collection, std::uint64_t seq) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_ledger_get(handle_, ns.c_str(), collection.c_str(), seq, &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// The head chain hash of ledger `collection` as an `"algo:hex"` string, or `std::nullopt` when absent or
    /// empty.
    std::optional<std::string> ledger_head(const std::string &ns, const std::string &collection) {
        char *out = nullptr;
        std::int32_t found = 0;
        detail::check(::loom_ledger_head(handle_, ns.c_str(), collection.c_str(), &out, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_string(out);
    }

    /// The number of entries in ledger `collection` (0 when absent).
    std::uint64_t ledger_len(const std::string &ns, const std::string &collection) {
        std::uint64_t out = 0;
        detail::check(::loom_ledger_len(handle_, ns.c_str(), collection.c_str(), &out));
        return out;
    }

    /// Recompute ledger `collection`'s chain from genesis and confirm every stored hash matches; an altered
    /// payload or broken link throws.
    void ledger_verify(const std::string &ns, const std::string &collection) {
        detail::check(::loom_ledger_verify(handle_, ns.c_str(), collection.c_str()));
    }

    /// Create (or replace the metadata of) calendar collection `collection` under `principal` in
    /// workspace `ns` (UUID or name, created with the `calendar` facet if absent). `display_name` is the
    /// collection's display name; `components` is a comma-separated component set ("event,todo"; "" is
    /// the empty set).
    void cal_create_collection(const std::string &ns, const std::string &principal,
                               const std::string &collection, const std::string &display_name,
                               const std::string &components) {
        detail::check(::loom_cal_create_collection(handle_, ns.c_str(), principal.c_str(),
                                                   collection.c_str(), display_name.c_str(),
                                                   components.c_str()));
    }

    /// Delete calendar collection `collection` under `principal` and every entry in it; returns whether
    /// it existed.
    bool cal_delete_collection(const std::string &ns, const std::string &principal,
                               const std::string &collection) {
        std::int32_t found = 0;
        detail::check(::loom_cal_delete_collection(handle_, ns.c_str(), principal.c_str(),
                                                   collection.c_str(), &found));
        return found != 0;
    }

    /// List the calendar collection ids under `principal` as the Loom Canonical CBOR array of text
    /// strings (sorted; an absent principal is the empty array).
    std::vector<std::uint8_t> cal_list_collections(const std::string &ns,
                                                   const std::string &principal) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_cal_list_collections(handle_, ns.c_str(), principal.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Put the calendar `entry` (its `CalendarEntry` canonical CBOR) into the existing collection
    /// `collection` under `principal`, keyed by its UID. A later put at the same UID replaces it.
    void cal_put_entry(const std::string &ns, const std::string &principal,
                       const std::string &collection, const std::vector<std::uint8_t> &entry) {
        detail::check(::loom_cal_put_entry(handle_, ns.c_str(), principal.c_str(), collection.c_str(),
                                           entry.data(), entry.size()));
    }

    /// Fetch the calendar entry at `uid` in collection `collection` as its `CalendarEntry` canonical
    /// CBOR, or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> cal_get_entry(const std::string &ns,
                                                           const std::string &principal,
                                                           const std::string &collection,
                                                           const std::string &uid) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_cal_get_entry(handle_, ns.c_str(), principal.c_str(), collection.c_str(),
                                           uid.c_str(), &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Remove the calendar entry at `uid` in collection `collection`; returns whether it was present.
    bool cal_delete_entry(const std::string &ns, const std::string &principal,
                          const std::string &collection, const std::string &uid) {
        std::int32_t found = 0;
        detail::check(::loom_cal_delete_entry(handle_, ns.c_str(), principal.c_str(),
                                              collection.c_str(), uid.c_str(), &found));
        return found != 0;
    }

    /// List collection `collection` as the Loom Canonical CBOR array of per-entry `CalendarEntry`
    /// canonical CBOR byte strings (UID order; an absent collection is the empty array).
    std::vector<std::uint8_t> cal_list_entries(const std::string &ns, const std::string &principal,
                                               const std::string &collection) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_cal_list_entries(handle_, ns.c_str(), principal.c_str(),
                                              collection.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Expand collection `collection` into occurrences within the half-open wall-clock window
    /// `[from, to)` (both `YYYYMMDDTHHMMSS`) as the Loom Canonical CBOR array of
    /// `[uid, "YYYYMMDDTHHMMSS"]` pairs.
    std::vector<std::uint8_t> cal_range(const std::string &ns, const std::string &principal,
                                        const std::string &collection, const std::string &from,
                                        const std::string &to) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_cal_range(handle_, ns.c_str(), principal.c_str(), collection.c_str(),
                                       from.c_str(), to.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Search collection `collection` by component filter (`""`/`"event"`/`"todo"`) and a
    /// case-insensitive summary substring `text` as the Loom Canonical CBOR array of per-entry
    /// `CalendarEntry` canonical CBOR byte strings.
    std::vector<std::uint8_t> cal_search(const std::string &ns, const std::string &principal,
                                         const std::string &collection, const std::string &component,
                                         const std::string &text) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_cal_search(handle_, ns.c_str(), principal.c_str(), collection.c_str(),
                                        component.c_str(), text.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The on-demand iCalendar (`.ics`) projection of the entry at `uid`, or `std::nullopt` if absent.
    std::optional<std::string> cal_entry_ics(const std::string &ns, const std::string &principal,
                                             const std::string &collection, const std::string &uid) {
        char *out = nullptr;
        std::int32_t found = 0;
        detail::check(::loom_cal_entry_ics(handle_, ns.c_str(), principal.c_str(), collection.c_str(),
                                           uid.c_str(), &out, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_string(out);
    }

    /// Parse iCalendar document `ics` and store it as a record in collection `collection`; returns the
    /// new ETag as a `"algo:hex"` string.
    std::string cal_put_ics(const std::string &ns, const std::string &principal,
                            const std::string &collection, const std::string &ics) {
        char *out = nullptr;
        detail::check(::loom_cal_put_ics(handle_, ns.c_str(), principal.c_str(), collection.c_str(),
                                         ics.c_str(), &out));
        return detail::take_string(out);
    }

    /// Create (or replace the metadata of) address book `book` under `principal` in workspace `ns`
    /// (UUID or name, created with the `contacts` facet if absent). `display_name` is the book's
    /// display name.
    void card_create_book(const std::string &ns, const std::string &principal,
                          const std::string &book, const std::string &display_name) {
        detail::check(::loom_card_create_book(handle_, ns.c_str(), principal.c_str(), book.c_str(),
                                              display_name.c_str()));
    }

    /// Delete address book `book` under `principal` and every contact in it; returns whether it existed.
    bool card_delete_book(const std::string &ns, const std::string &principal,
                          const std::string &book) {
        std::int32_t found = 0;
        detail::check(
            ::loom_card_delete_book(handle_, ns.c_str(), principal.c_str(), book.c_str(), &found));
        return found != 0;
    }

    /// List the address-book ids under `principal` as the Loom Canonical CBOR array of text strings
    /// (sorted; an absent principal is the empty array).
    std::vector<std::uint8_t> card_list_books(const std::string &ns, const std::string &principal) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_card_list_books(handle_, ns.c_str(), principal.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Put the contact `entry` (its `ContactEntry` canonical CBOR) into the existing address book `book`
    /// under `principal`, keyed by its UID. A later put at the same UID replaces it.
    void card_put_entry(const std::string &ns, const std::string &principal, const std::string &book,
                        const std::vector<std::uint8_t> &entry) {
        detail::check(::loom_card_put_entry(handle_, ns.c_str(), principal.c_str(), book.c_str(),
                                            entry.data(), entry.size()));
    }

    /// Fetch the contact at `uid` in address book `book` as its `ContactEntry` canonical CBOR, or
    /// `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> card_get_entry(const std::string &ns,
                                                            const std::string &principal,
                                                            const std::string &book,
                                                            const std::string &uid) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_card_get_entry(handle_, ns.c_str(), principal.c_str(), book.c_str(),
                                            uid.c_str(), &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Remove the contact at `uid` in address book `book`; returns whether it was present.
    bool card_delete_entry(const std::string &ns, const std::string &principal,
                           const std::string &book, const std::string &uid) {
        std::int32_t found = 0;
        detail::check(::loom_card_delete_entry(handle_, ns.c_str(), principal.c_str(), book.c_str(),
                                               uid.c_str(), &found));
        return found != 0;
    }

    /// List address book `book` as the Loom Canonical CBOR array of per-contact `ContactEntry`
    /// canonical CBOR byte strings (UID order; an absent book is the empty array).
    std::vector<std::uint8_t> card_list_entries(const std::string &ns, const std::string &principal,
                                                const std::string &book) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_card_list_entries(handle_, ns.c_str(), principal.c_str(), book.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Search address book `book` by a case-insensitive substring `text` over the formatted name,
    /// organization, and email values as the Loom Canonical CBOR array of per-contact `ContactEntry`
    /// canonical CBOR byte strings.
    std::vector<std::uint8_t> card_search(const std::string &ns, const std::string &principal,
                                          const std::string &book, const std::string &text) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_card_search(handle_, ns.c_str(), principal.c_str(), book.c_str(),
                                         text.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The on-demand vCard (`.vcf`) projection of the contact at `uid`, or `std::nullopt` if absent.
    std::optional<std::string> card_entry_vcard(const std::string &ns, const std::string &principal,
                                                const std::string &book, const std::string &uid) {
        char *out = nullptr;
        std::int32_t found = 0;
        detail::check(::loom_card_entry_vcard(handle_, ns.c_str(), principal.c_str(), book.c_str(),
                                              uid.c_str(), &out, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_string(out);
    }

    /// Parse vCard document `vcf` and store it as a record in address book `book`; returns the new ETag
    /// as a `"algo:hex"` string.
    std::string card_put_vcard(const std::string &ns, const std::string &principal,
                               const std::string &book, const std::string &vcf) {
        char *out = nullptr;
        detail::check(::loom_card_put_vcard(handle_, ns.c_str(), principal.c_str(), book.c_str(),
                                            vcf.c_str(), &out));
        return detail::take_string(out);
    }

    /// Create (or replace the metadata of) mailbox `mailbox` under `principal` in workspace `ns` (UUID
    /// or name, created with the `mail` facet if absent). `display_name` is the mailbox's display name.
    void mail_create_mailbox(const std::string &ns, const std::string &principal,
                             const std::string &mailbox, const std::string &display_name) {
        detail::check(::loom_mail_create_mailbox(handle_, ns.c_str(), principal.c_str(),
                                                 mailbox.c_str(), display_name.c_str()));
    }

    /// Delete mailbox `mailbox` under `principal` and every message index and flag set in it (immutable
    /// bodies stay in the CAS until GC); returns whether it existed.
    bool mail_delete_mailbox(const std::string &ns, const std::string &principal,
                             const std::string &mailbox) {
        std::int32_t found = 0;
        detail::check(::loom_mail_delete_mailbox(handle_, ns.c_str(), principal.c_str(),
                                                 mailbox.c_str(), &found));
        return found != 0;
    }

    /// List the mailbox ids under `principal` as the Loom Canonical CBOR array of text strings (sorted;
    /// an absent principal is the empty array).
    std::vector<std::uint8_t> mail_list_mailboxes(const std::string &ns,
                                                  const std::string &principal) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_mail_list_mailboxes(handle_, ns.c_str(), principal.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Ingest the raw RFC 5322 message `raw` into mailbox `mailbox` under `uid` (store the immutable
    /// body in the CAS, parse the headers into a structured index, write it); returns the body's
    /// content address as a `"algo:hex"` string.
    std::string mail_ingest_message(const std::string &ns, const std::string &principal,
                                    const std::string &mailbox, const std::string &uid,
                                    const std::vector<std::uint8_t> &raw) {
        char *out = nullptr;
        detail::check(::loom_mail_ingest_message(handle_, ns.c_str(), principal.c_str(),
                                                 mailbox.c_str(), uid.c_str(), raw.data(), raw.size(),
                                                 &out));
        return detail::take_string(out);
    }

    /// Fetch the structured index of the message at `uid` in mailbox `mailbox` as its `MailMessage`
    /// canonical CBOR, or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> mail_get_message(const std::string &ns,
                                                              const std::string &principal,
                                                              const std::string &mailbox,
                                                              const std::string &uid) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_mail_get_message(handle_, ns.c_str(), principal.c_str(), mailbox.c_str(),
                                              uid.c_str(), &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Fetch the raw RFC 5322 body (`.eml` bytes) of the message at `uid`, from the CAS and
    /// digest-verified, or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> mail_to_eml(const std::string &ns,
                                                           const std::string &principal,
                                                           const std::string &mailbox,
                                                           const std::string &uid) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_mail_to_eml(handle_, ns.c_str(), principal.c_str(), mailbox.c_str(),
                                           uid.c_str(), &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Remove the message index and its flags at `uid` (the immutable body stays in the CAS until GC);
    /// returns whether it was present.
    bool mail_delete_message(const std::string &ns, const std::string &principal,
                             const std::string &mailbox, const std::string &uid) {
        std::int32_t found = 0;
        detail::check(::loom_mail_delete_message(handle_, ns.c_str(), principal.c_str(),
                                                 mailbox.c_str(), uid.c_str(), &found));
        return found != 0;
    }

    /// List mailbox `mailbox` as the Loom Canonical CBOR array of per-message `MailMessage` canonical
    /// CBOR byte strings (UID order; an absent mailbox is the empty array).
    std::vector<std::uint8_t> mail_list_messages(const std::string &ns, const std::string &principal,
                                                 const std::string &mailbox) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_mail_list_messages(handle_, ns.c_str(), principal.c_str(),
                                                mailbox.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The flags/labels on the message at `uid` as the Loom Canonical CBOR array of text strings
    /// (sorted, deduplicated; an absent flag set is the empty array).
    std::vector<std::uint8_t> mail_get_flags(const std::string &ns, const std::string &principal,
                                             const std::string &mailbox, const std::string &uid) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_mail_get_flags(handle_, ns.c_str(), principal.c_str(), mailbox.c_str(),
                                            uid.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Replace the flags/labels on the message at `uid` with `flags`, a Loom Canonical CBOR
    /// `Array(Text)` buffer. The message must exist.
    void mail_set_flags(const std::string &ns, const std::string &principal,
                        const std::string &mailbox, const std::string &uid,
                        const std::vector<std::uint8_t> &flags) {
        detail::check(::loom_mail_set_flags(handle_, ns.c_str(), principal.c_str(), mailbox.c_str(),
                                            uid.c_str(), flags.data(), flags.size()));
    }

    /// Search mailbox `mailbox` by a case-insensitive substring `text` over the subject and from
    /// values as the Loom Canonical CBOR array of per-message `MailMessage` canonical CBOR byte strings.
    std::vector<std::uint8_t> mail_search(const std::string &ns, const std::string &principal,
                                          const std::string &mailbox, const std::string &text) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_mail_search(handle_, ns.c_str(), principal.c_str(), mailbox.c_str(),
                                         text.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Append `entry` to `stream` in workspace `ns` (by UUID or name, created with the queue facet if
    /// absent); returns the assigned zero-based sequence.
    std::uint64_t queue_append(const std::string &ns, const std::string &stream,
                               const std::vector<std::uint8_t> &entry) {
        std::uint64_t seq = 0;
        detail::check(::loom_queue_append(handle_, ns.c_str(), stream.c_str(), entry.data(),
                                          entry.size(), &seq));
        return seq;
    }

    /// Fetch the entry at `seq` from `stream` in workspace `ns`, or `std::nullopt` if out of range.
    std::optional<std::vector<std::uint8_t>> queue_get(const std::string &ns,
                                                       const std::string &stream,
                                                       std::uint64_t seq) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(
            ::loom_queue_get(handle_, ns.c_str(), stream.c_str(), seq, &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// The number of entries in `stream` of workspace `ns`.
    std::uint64_t queue_len(const std::string &ns, const std::string &stream) {
        std::uint64_t len = 0;
        detail::check(::loom_queue_len(handle_, ns.c_str(), stream.c_str(), &len));
        return len;
    }

    /// The half-open range `[lo, hi)` of `stream` in workspace `ns` as raw Loom Canonical CBOR (an
    /// array of byte strings). Typed C++ decoding of the array is out of scope.
    std::vector<std::uint8_t> queue_range_cbor(const std::string &ns, const std::string &stream,
                                               std::uint64_t lo, std::uint64_t hi) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_queue_range(handle_, ns.c_str(), stream.c_str(), lo, hi, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The named consumer's next sequence for `stream`; `0` when none is stored.
    std::uint64_t queue_consumer_position(const std::string &ns, const std::string &stream,
                                          const std::string &consumer_id) {
        std::uint64_t seq = 0;
        detail::check(::loom_queue_consumer_position(handle_, ns.c_str(), stream.c_str(),
                                                     consumer_id.c_str(), &seq));
        return seq;
    }

    /// Up to `max` entries from the consumer's stored next sequence as raw Loom Canonical CBOR (an
    /// array of byte strings); does not advance progress. Typed C++ decoding is out of scope.
    std::vector<std::uint8_t> queue_consumer_read_cbor(const std::string &ns,
                                                       const std::string &stream,
                                                       const std::string &consumer_id,
                                                       std::uint32_t max) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_queue_consumer_read(handle_, ns.c_str(), stream.c_str(),
                                                 consumer_id.c_str(), max, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Advance the named consumer's next sequence for `stream` to `next_seq`; rejects backward movement.
    void queue_consumer_advance(const std::string &ns, const std::string &stream,
                                const std::string &consumer_id, std::uint64_t next_seq) {
        detail::check(::loom_queue_consumer_advance(handle_, ns.c_str(), stream.c_str(),
                                                    consumer_id.c_str(), next_seq));
    }

    /// Set the named consumer's next sequence for `stream` to `next_seq`, which may move backward.
    void queue_consumer_reset(const std::string &ns, const std::string &stream,
                              const std::string &consumer_id, std::uint64_t next_seq) {
        detail::check(::loom_queue_consumer_reset(handle_, ns.c_str(), stream.c_str(),
                                                  consumer_id.c_str(), next_seq));
    }

    /// Insert or replace node `id` in graph `name` of workspace `ns` (created with the `graph` facet if
    /// absent). `props` is a Loom Canonical CBOR `text -> bytes` map (empty = no properties). A later
    /// upsert at the same id replaces it.
    void graph_upsert_node(const std::string &ns, const std::string &name, const std::string &id,
                           const std::vector<std::uint8_t> &props) {
        detail::check(::loom_graph_upsert_node(handle_, ns.c_str(), name.c_str(), id.c_str(),
                                               props.data(), props.size()));
    }

    /// Fetch the props of node `id` in graph `name` as a Loom Canonical CBOR `text -> bytes` map, or
    /// `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> graph_get_node(const std::string &ns,
                                                            const std::string &name,
                                                            const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_graph_get_node(handle_, ns.c_str(), name.c_str(), id.c_str(), &ptr, &len,
                                            &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Remove node `id` from graph `name`. With `cascade` true, also removes incident edges; otherwise
    /// throws `CONFLICT` while any incident edge exists.
    void graph_remove_node(const std::string &ns, const std::string &name, const std::string &id,
                           bool cascade = false) {
        detail::check(::loom_graph_remove_node(handle_, ns.c_str(), name.c_str(), id.c_str(),
                                               cascade ? 1 : 0));
    }

    /// Insert or replace edge `id` from `src` to `dst` (both must exist) with `label` and CBOR `props`
    /// (a `text -> bytes` map, empty = none) in graph `name`. A later upsert at the same id replaces it.
    void graph_upsert_edge(const std::string &ns, const std::string &name, const std::string &id,
                           const std::string &src, const std::string &dst, const std::string &label,
                           const std::vector<std::uint8_t> &props) {
        detail::check(::loom_graph_upsert_edge(handle_, ns.c_str(), name.c_str(), id.c_str(),
                                               src.c_str(), dst.c_str(), label.c_str(), props.data(),
                                               props.size()));
    }

    /// Fetch edge `id` in graph `name` as the Loom Canonical CBOR array `[src, dst, label, props]`, or
    /// `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> graph_get_edge(const std::string &ns,
                                                            const std::string &name,
                                                            const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_graph_get_edge(handle_, ns.c_str(), name.c_str(), id.c_str(), &ptr, &len,
                                            &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Remove edge `id` from graph `name`; returns whether it was present.
    bool graph_remove_edge(const std::string &ns, const std::string &name, const std::string &id) {
        std::int32_t found = 0;
        detail::check(
            ::loom_graph_remove_edge(handle_, ns.c_str(), name.c_str(), id.c_str(), &found));
        return found != 0;
    }

    /// The distinct adjacent node ids of `id` in graph `name`, sorted, as a Loom Canonical CBOR array
    /// of text.
    std::vector<std::uint8_t> graph_neighbors(const std::string &ns, const std::string &name,
                                              const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_graph_neighbors_cbor(handle_, ns.c_str(), name.c_str(), id.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The out-edges of `id` in graph `name` as a Loom Canonical CBOR array of `[edge_id, edge]` in
    /// edge-id order.
    std::vector<std::uint8_t> graph_out_edges(const std::string &ns, const std::string &name,
                                              const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_graph_out_edges_cbor(handle_, ns.c_str(), name.c_str(), id.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The in-edges of `id` in graph `name` as a Loom Canonical CBOR array of `[edge_id, edge]` in
    /// edge-id order.
    std::vector<std::uint8_t> graph_in_edges(const std::string &ns, const std::string &name,
                                             const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_graph_in_edges_cbor(handle_, ns.c_str(), name.c_str(), id.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The node ids reachable from `start` in graph `name` as a Loom Canonical CBOR array of text.
    /// `max_depth` below zero means no limit; a `via_label` follows only edges with that label, else
    /// `std::nullopt` follows every edge.
    std::vector<std::uint8_t> graph_reachable(const std::string &ns, const std::string &name,
                                              const std::string &start, std::int64_t max_depth = -1,
                                              const std::optional<std::string> &via_label = std::nullopt) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_graph_reachable_cbor(handle_, ns.c_str(), name.c_str(), start.c_str(),
                                                  max_depth, via_label ? via_label->c_str() : nullptr,
                                                  &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// A shortest path from `from` to `to` in graph `name` as a Loom Canonical CBOR array of node-id
    /// text, or `std::nullopt` if no path exists. A `via_label` follows only edges with that label, else
    /// `std::nullopt` follows every edge.
    std::optional<std::vector<std::uint8_t>>
    graph_shortest_path(const std::string &ns, const std::string &name, const std::string &from,
                        const std::string &to,
                        const std::optional<std::string> &via_label = std::nullopt) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_graph_shortest_path_cbor(handle_, ns.c_str(), name.c_str(), from.c_str(),
                                                      to.c_str(),
                                                      via_label ? via_label->c_str() : nullptr, &ptr,
                                                      &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Create vector set `name` of width `dim` and `metric` (1 cosine, 2 L2, 3 dot) in workspace `ns`
    /// (created with the `vector` facet if absent). Throws `CONFLICT` if the set already exists.
    void vector_create(const std::string &ns, const std::string &name, std::uintptr_t dim,
                       std::int32_t metric) {
        detail::check(::loom_vector_create(handle_, ns.c_str(), name.c_str(), dim, metric));
    }

    /// Insert or replace the vector at `id` in set `name`. `vector` is little-endian f32 bytes (4 per
    /// component); `metadata` is a Loom Canonical CBOR `text -> cell` map (empty = none). A later upsert
    /// at the same id replaces it.
    void vector_upsert(const std::string &ns, const std::string &name, const std::string &id,
                       const std::vector<std::uint8_t> &vector,
                       const std::vector<std::uint8_t> &metadata) {
        detail::check(::loom_vector_upsert(handle_, ns.c_str(), name.c_str(), id.c_str(),
                                           vector.data(), vector.size(), metadata.data(),
                                           metadata.size()));
    }

    /// Insert or replace a vector with UTF-8 source text and optional embedding model profile.
    void vector_upsert_source(const std::string &ns, const std::string &name,
                              const std::string &id, const std::vector<std::uint8_t> &vector,
                              const std::vector<std::uint8_t> &metadata,
                              const std::string &source_text,
                              const std::optional<std::string> &model_id = std::nullopt,
                              const std::optional<std::string> &weights_digest = std::nullopt) {
        detail::check(::loom_vector_upsert_source(
            handle_, ns.c_str(), name.c_str(), id.c_str(), vector.data(), vector.size(),
            metadata.data(), metadata.size(),
            reinterpret_cast<const unsigned char *>(source_text.data()), source_text.size(),
            model_id ? model_id->c_str() : nullptr, model_id ? 1 : 0,
            weights_digest ? weights_digest->c_str() : nullptr, weights_digest ? 1 : 0));
    }

    /// Fetch the vector and metadata at `id` in set `name` as the Loom Canonical CBOR array
    /// `[vector_bytes, metadata]`, or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> vector_get(const std::string &ns, const std::string &name,
                                                        const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_vector_get(handle_, ns.c_str(), name.c_str(), id.c_str(), &ptr, &len,
                                        &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Fetch UTF-8 source text for vector `id`, or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> vector_source_text(const std::string &ns,
                                                                const std::string &name,
                                                                const std::string &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_vector_source_text(handle_, ns.c_str(), name.c_str(), id.c_str(),
                                                &ptr, &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Fetch the embedding model profile as CBOR `[1, model_id, dimension, weights_digest]`.
    std::optional<std::vector<std::uint8_t>> vector_embedding_model(const std::string &ns,
                                                                    const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_vector_embedding_model_cbor(handle_, ns.c_str(), name.c_str(), &ptr,
                                                         &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Vector ids in set `name`, sorted ascending, as a Loom Canonical CBOR array of text. When
    /// `prefix` is present, only ids starting with that string prefix are returned.
    std::vector<std::uint8_t> vector_ids(
        const std::string &ns, const std::string &name,
        const std::optional<std::string> &prefix = std::nullopt) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_vector_ids_cbor(handle_, ns.c_str(), name.c_str(),
                                             prefix ? prefix->c_str() : nullptr, prefix ? 1 : 0, &ptr,
                                             &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Declared metadata equality index keys for set `name`, sorted ascending, as a Loom Canonical CBOR
    /// array of text.
    std::vector<std::uint8_t> vector_metadata_index_keys(const std::string &ns,
                                                         const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_vector_metadata_index_keys_cbor(handle_, ns.c_str(), name.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Declare and build a metadata equality index for `key`; returns whether a new index was
    /// declared.
    bool vector_create_metadata_index(const std::string &ns, const std::string &name,
                                      const std::string &key) {
        std::int32_t changed = 0;
        detail::check(::loom_vector_create_metadata_index(handle_, ns.c_str(), name.c_str(),
                                                          key.c_str(), &changed));
        return changed != 0;
    }

    /// Drop the metadata equality index for `key`; returns whether an index was present.
    bool vector_drop_metadata_index(const std::string &ns, const std::string &name,
                                    const std::string &key) {
        std::int32_t changed = 0;
        detail::check(::loom_vector_drop_metadata_index(handle_, ns.c_str(), name.c_str(),
                                                        key.c_str(), &changed));
        return changed != 0;
    }

    /// Remove the vector at `id` from set `name`; returns whether it was present.
    bool vector_delete(const std::string &ns, const std::string &name, const std::string &id) {
        std::int32_t found = 0;
        detail::check(::loom_vector_delete(handle_, ns.c_str(), name.c_str(), id.c_str(), &found));
        return found != 0;
    }

    /// The exact top-`k` nearest neighbours of `query` (little-endian f32 bytes) in set `name` among
    /// vectors passing `filter`, as a Loom Canonical CBOR array of `[id, score_cell]`, highest score
    /// first. An empty `filter` matches all.
    std::vector<std::uint8_t> vector_search(const std::string &ns, const std::string &name,
                                            const std::vector<std::uint8_t> &query, std::uintptr_t k,
                                            const std::vector<std::uint8_t> &filter) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_vector_search_cbor(handle_, ns.c_str(), name.c_str(), query.data(),
                                                query.size(), k, filter.data(), filter.size(), &ptr,
                                                &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Top-k vector search with explicit accelerator policy over the built-in PQ accelerator.
    /// `policy` is 0 exact, 1 approximate-above-threshold. Result CBOR matches `vector_search`.
    std::vector<std::uint8_t> vector_search_policy(
        const std::string &ns, const std::string &name, const std::vector<std::uint8_t> &query,
        std::uintptr_t k, const std::vector<std::uint8_t> &filter, std::int32_t policy,
        std::uintptr_t threshold, std::uintptr_t ef, std::uintptr_t pq_m, std::uintptr_t pq_k,
        std::uintptr_t pq_iters) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_vector_search_policy_cbor(
            handle_, ns.c_str(), name.c_str(), query.data(), query.size(), k, filter.data(),
            filter.size(), policy, threshold, ef, pq_m, pq_k, pq_iters, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Create columnar dataset `name` in workspace `ns` (created with the `columnar` facet if absent).
    /// `columns` is a Loom Canonical CBOR array of `[name, type_tag]`; `target_segment_rows` of 0 uses
    /// the default.
    void columnar_create(const std::string &ns, const std::string &name,
                         const std::vector<std::uint8_t> &columns,
                         std::uintptr_t target_segment_rows = 0) {
        detail::check(::loom_columnar_create(handle_, ns.c_str(), name.c_str(), columns.data(),
                                             columns.size(), target_segment_rows));
    }

    /// Append `row` (a Loom Canonical CBOR cell array) to dataset `name`, validating arity and column
    /// types.
    void columnar_append(const std::string &ns, const std::string &name,
                         const std::vector<std::uint8_t> &row) {
        detail::check(
            ::loom_columnar_append(handle_, ns.c_str(), name.c_str(), row.data(), row.size()));
    }

    /// All rows of dataset `name` in append order as a Loom Canonical CBOR array of cell arrays.
    std::vector<std::uint8_t> columnar_scan(const std::string &ns, const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_columnar_scan_cbor(handle_, ns.c_str(), name.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The columns of dataset `name` as a Loom Canonical CBOR array of `[name, type_tag]`.
    std::vector<std::uint8_t> columnar_columns(const std::string &ns, const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_columnar_columns_cbor(handle_, ns.c_str(), name.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// The total row count of dataset `name`.
    std::uint64_t columnar_rows(const std::string &ns, const std::string &name) {
        std::uint64_t out = 0;
        detail::check(::loom_columnar_rows(handle_, ns.c_str(), name.c_str(), &out));
        return out;
    }

    /// Compact dataset `name` at its target segment size.
    void columnar_compact(const std::string &ns, const std::string &name) {
        detail::check(::loom_columnar_compact(handle_, ns.c_str(), name.c_str()));
    }

    /// Inspect dataset metadata as a Loom Canonical CBOR array.
    std::vector<std::uint8_t> columnar_inspect(const std::string &ns, const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_columnar_inspect_cbor(handle_, ns.c_str(), name.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Source digest used by derived columnar projections as CBOR text.
    std::vector<std::uint8_t> columnar_source_digest(const std::string &ns,
                                                     const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_columnar_source_digest_cbor(handle_, ns.c_str(), name.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Project `columns` (a Loom Canonical CBOR array of text) from rows of dataset `name` matching
    /// `filter` (the CBOR array `[col, op, value_cell]`; empty = all) as a Loom Canonical CBOR array of
    /// cell arrays.
    std::vector<std::uint8_t> columnar_select(const std::string &ns, const std::string &name,
                                              const std::vector<std::uint8_t> &columns,
                                              const std::vector<std::uint8_t> &filter) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_columnar_select_cbor(handle_, ns.c_str(), name.c_str(), columns.data(),
                                                  columns.size(), filter.data(), filter.size(), &ptr,
                                                  &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Evaluate aggregate expressions from CBOR `[[op, column?] ...]`, with optional select filter.
    std::vector<std::uint8_t> columnar_aggregate(const std::string &ns, const std::string &name,
                                                 const std::vector<std::uint8_t> &aggregates,
                                                 const std::vector<std::uint8_t> &filter) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_columnar_aggregate_cbor(handle_, ns.c_str(), name.c_str(),
                                                     aggregates.data(), aggregates.size(),
                                                     filter.data(), filter.size(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Create dataframe frame `name` in workspace `ns` from canonical `DataframePlan` CBOR.
    void dataframe_create(const std::string &ns, const std::string &name,
                          const std::vector<std::uint8_t> &plan) {
        detail::check(::loom_dataframe_create(handle_, ns.c_str(), name.c_str(), plan.data(),
                                              plan.size()));
    }

    /// Execute dataframe frame `name` as canonical CBOR `[columns, rows]`.
    std::vector<std::uint8_t> dataframe_collect(const std::string &ns, const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_dataframe_collect_cbor(handle_, ns.c_str(), name.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Execute dataframe frame `name` and return at most `rows` rows as canonical CBOR `[columns, rows]`.
    std::vector<std::uint8_t> dataframe_preview(const std::string &ns, const std::string &name,
                                                std::uint64_t rows) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_dataframe_preview_cbor(handle_, ns.c_str(), name.c_str(), rows, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Materialize dataframe frame `name`; returns a CAS digest when the materialization target emits one.
    std::optional<std::string> dataframe_materialize(const std::string &ns,
                                                     const std::string &name) {
        char *out = nullptr;
        std::int32_t has_digest = 0;
        detail::check(
            ::loom_dataframe_materialize(handle_, ns.c_str(), name.c_str(), &out, &has_digest));
        if (has_digest == 0) {
            return std::nullopt;
        }
        return detail::take_string(out);
    }

    /// Canonical dataframe plan digest as `algo:hex`.
    std::string dataframe_plan_digest(const std::string &ns, const std::string &name) {
        char *out = nullptr;
        detail::check(::loom_dataframe_plan_digest(handle_, ns.c_str(), name.c_str(), &out));
        return detail::take_string(out);
    }

    /// Source digests pinned in the dataframe plan as canonical CBOR array of `algo:hex` strings.
    std::vector<std::uint8_t> dataframe_source_digests(const std::string &ns,
                                                       const std::string &name) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_dataframe_source_digests_cbor(handle_, ns.c_str(), name.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Create search collection `name` in workspace `ns` (created with the `search` facet if absent).
    /// `mapping` is a Loom Canonical CBOR map of `field -> [type_tag, stored, faceted]`. Throws
    /// `CONFLICT` if the collection already exists.
    void search_create(const std::string &ns, const std::string &name,
                       const std::vector<std::uint8_t> &mapping) {
        detail::check(::loom_search_create(handle_, ns.c_str(), name.c_str(), mapping.data(),
                                           mapping.size()));
    }

    /// Insert or replace the document at `id` (opaque bytes) in collection `name`. `doc` is a Loom
    /// Canonical CBOR `field -> value` map (text or bytes). A later index at the same id replaces it.
    void search_index(const std::string &ns, const std::string &name,
                      const std::vector<std::uint8_t> &id, const std::vector<std::uint8_t> &doc) {
        detail::check(::loom_search_index(handle_, ns.c_str(), name.c_str(), id.data(), id.size(),
                                          doc.data(), doc.size()));
    }

    /// Fetch the document at `id` in collection `name` as a Loom Canonical CBOR `field -> value` map,
    /// or `std::nullopt` if absent.
    std::optional<std::vector<std::uint8_t>> search_get(const std::string &ns, const std::string &name,
                                                        const std::vector<std::uint8_t> &id) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        std::int32_t found = 0;
        detail::check(::loom_search_get(handle_, ns.c_str(), name.c_str(), id.data(), id.size(), &ptr,
                                        &len, &found));
        if (found == 0) {
            return std::nullopt;
        }
        return detail::take_result_bytes(ptr, len);
    }

    /// Remove the document at `id` from collection `name`; returns whether it was present.
    bool search_delete(const std::string &ns, const std::string &name,
                       const std::vector<std::uint8_t> &id) {
        std::int32_t found = 0;
        detail::check(
            ::loom_search_delete(handle_, ns.c_str(), name.c_str(), id.data(), id.size(), &found));
        return found != 0;
    }

    /// The document ids of collection `name` as a Loom Canonical CBOR array of byte strings. A `prefix`
    /// restricts the result to ids under it; `std::nullopt` lists all.
    std::vector<std::uint8_t>
    search_ids(const std::string &ns, const std::string &name,
               const std::optional<std::vector<std::uint8_t>> &prefix = std::nullopt) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_search_ids_cbor(handle_, ns.c_str(), name.c_str(),
                                             prefix ? prefix->data() : nullptr,
                                             prefix ? prefix->size() : 0, prefix ? 1 : 0, &ptr,
                                             &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Replace the field mapping of collection `name` with `mapping`, a Loom Canonical CBOR map of
    /// `field -> [type_tag, stored, faceted]`.
    void search_remap(const std::string &ns, const std::string &name,
                      const std::vector<std::uint8_t> &mapping) {
        detail::check(::loom_search_remap(handle_, ns.c_str(), name.c_str(), mapping.data(),
                                          mapping.size()));
    }

    /// Run the portable linear-scan query `request` (a Loom Canonical CBOR `[query, limit, offset]`)
    /// over collection `name`; returns the response as Loom Canonical CBOR.
    std::vector<std::uint8_t> search_query(const std::string &ns, const std::string &name,
                                           const std::vector<std::uint8_t> &request) {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_search_query_cbor(handle_, ns.c_str(), name.c_str(), request.data(),
                                               request.size(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Read a staged table from the SQL facet of workspace `ns_name`.
    result sql_read_table(const std::string &ns_name, const std::string &table) const {
        auto bytes = sql_read_table_bytes(ns_name, table);
        return open_result(bytes);
    }

    /// Read a staged table as canonical CBOR bytes.
    std::vector<std::uint8_t> sql_read_table_bytes(const std::string &ns_name,
                                                   const std::string &table) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(
            ::loom_sql_read_table(handle_, ns_name.c_str(), table.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Read a table from a historical commit.
    result sql_read_table_at(const std::string &ns_name, const std::string &table,
                             const std::string &commit) const {
        auto bytes = sql_read_table_at_bytes(ns_name, table, commit);
        return open_result(bytes);
    }

    /// Read a historical table as canonical CBOR bytes.
    std::vector<std::uint8_t> sql_read_table_at_bytes(const std::string &ns_name,
                                                      const std::string &table,
                                                      const std::string &commit) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_read_table_at(handle_, ns_name.c_str(), table.c_str(),
                                               commit.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Scan secondary index `index` with a canonical-CBOR cell-array lookup prefix.
    result sql_index_scan(const std::string &ns_name, const std::string &table,
                          const std::string &index,
                          const std::vector<std::uint8_t> &prefix) const {
        auto bytes = sql_index_scan_bytes(ns_name, table, index, prefix);
        return open_result(bytes);
    }

    /// Scan a secondary index as canonical CBOR bytes.
    std::vector<std::uint8_t> sql_index_scan_bytes(const std::string &ns_name,
                                                   const std::string &table,
                                                   const std::string &index,
                                                   const std::vector<std::uint8_t> &prefix) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_index_scan(handle_, ns_name.c_str(), table.c_str(),
                                            index.c_str(), prefix.data(), prefix.size(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Scan a secondary index from a historical commit.
    result sql_index_scan_at(const std::string &ns_name, const std::string &table,
                             const std::string &index,
                             const std::vector<std::uint8_t> &prefix,
                             const std::string &commit) const {
        auto bytes = sql_index_scan_at_bytes(ns_name, table, index, prefix, commit);
        return open_result(bytes);
    }

    /// Scan a historical secondary index as canonical CBOR bytes.
    std::vector<std::uint8_t> sql_index_scan_at_bytes(const std::string &ns_name,
                                                      const std::string &table,
                                                      const std::string &index,
                                                      const std::vector<std::uint8_t> &prefix,
                                                      const std::string &commit) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_index_scan_at(handle_, ns_name.c_str(), table.c_str(),
                                               index.c_str(), prefix.data(), prefix.size(),
                                               commit.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Blame current rows in `table` on `branch`.
    result sql_blame(const std::string &ns_name, const std::string &branch,
                     const std::string &table) const {
        auto bytes = sql_blame_bytes(ns_name, branch, table);
        return open_result(bytes);
    }

    /// Blame current rows as canonical CBOR bytes.
    std::vector<std::uint8_t> sql_blame_bytes(const std::string &ns_name,
                                              const std::string &branch,
                                              const std::string &table) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_blame(handle_, ns_name.c_str(), branch.c_str(),
                                       table.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Row-level table diff between two commit addresses.
    result sql_diff(const std::string &ns_name, const std::string &table,
                    const std::string &from_commit, const std::string &to_commit) const {
        auto bytes = sql_diff_bytes(ns_name, table, from_commit, to_commit);
        return open_result(bytes);
    }

    /// Row-level table diff as canonical CBOR bytes.
    std::vector<std::uint8_t> sql_diff_bytes(const std::string &ns_name,
                                             const std::string &table,
                                             const std::string &from_commit,
                                             const std::string &to_commit) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_diff(handle_, ns_name.c_str(), table.c_str(),
                                      from_commit.c_str(), to_commit.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Schema-aware table diff as canonical CBOR bytes.
    std::vector<std::uint8_t> sql_table_diff_bytes(const std::string &ns_name,
                                                   const std::string &table,
                                                   const std::string &from_commit,
                                                   const std::string &to_commit) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_sql_table_diff(handle_, ns_name.c_str(), table.c_str(),
                                            from_commit.c_str(), to_commit.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Workspace/entry-level blame for `branch` (which commit last set each path).
    result vcs_blame(const std::string &ns_name, const std::string &branch) const {
        return open_result(vcs_blame_bytes(ns_name, branch));
    }

    /// Workspace/entry-level blame as canonical CBOR bytes.
    std::vector<std::uint8_t> vcs_blame_bytes(const std::string &ns_name,
                                              const std::string &branch) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_vcs_blame(handle_, ns_name.c_str(), branch.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Structural diff between two commit addresses.
    result vcs_diff(const std::string &ns_name, const std::string &from_commit,
                    const std::string &to_commit) const {
        return open_result(vcs_diff_bytes(ns_name, from_commit, to_commit));
    }

    /// Structural diff as LMDIFF canonical CBOR bytes.
    std::vector<std::uint8_t> vcs_diff_bytes(const std::string &ns_name,
                                             const std::string &from_commit,
                                             const std::string &to_commit) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_vcs_diff(handle_, ns_name.c_str(), from_commit.c_str(),
                                      to_commit.c_str(), &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

    /// Subscribe to workspace history changes and return an opaque watch cursor string.
    std::string watch_subscribe(
        const std::string &ns_name, const std::string &branch,
        const std::optional<std::string> &facet = std::nullopt,
        const std::optional<std::string> &path_prefix = std::nullopt,
        const std::vector<std::string> &change_kinds = {},
        const std::optional<std::string> &from_commit = std::nullopt) const {
        std::string kinds;
        for (const auto &kind : change_kinds) {
            if (!kinds.empty()) {
                kinds.push_back(',');
            }
            kinds += kind;
        }
        char *cursor = nullptr;
        detail::check(::loom_watch_subscribe(
            handle_, ns_name.c_str(), branch.c_str(), facet ? facet->c_str() : nullptr,
            path_prefix ? path_prefix->c_str() : nullptr,
            kinds.empty() ? nullptr : kinds.c_str(), from_commit ? from_commit->c_str() : nullptr,
            &cursor));
        return detail::take_string(cursor);
    }

    /// Poll an opaque watch cursor and return a canonical-CBOR `loom.watch.batch.v1` batch.
    std::vector<std::uint8_t> watch_poll_bytes(const std::string &cursor,
                                               std::uint32_t max) const {
        std::uint8_t *ptr = nullptr;
        std::uintptr_t len = 0;
        detail::check(::loom_watch_poll(handle_, cursor.c_str(), max, &ptr, &len));
        return detail::take_result_bytes(ptr, len);
    }

private:
    struct packed_acl_scopes {
        std::vector<std::int32_t> kinds;
        std::vector<const unsigned char *> prefixes;
        std::vector<std::uintptr_t> lens;
    };

    static packed_acl_scopes pack_acl_scopes(const std::vector<acl_scope> &scopes) {
        packed_acl_scopes packed;
        packed.kinds.reserve(scopes.size());
        packed.prefixes.reserve(scopes.size());
        packed.lens.reserve(scopes.size());
        for (const auto &scope : scopes) {
            packed.kinds.push_back(scope.kind);
            packed.prefixes.push_back(scope.prefix.empty() ? nullptr : scope.prefix.data());
            packed.lens.push_back(scope.prefix.size());
        }
        return packed;
    }

    static result open_result(const std::vector<std::uint8_t> &bytes) {
        LoomResultView *view = nullptr;
        detail::check(::loom_result_open(bytes.data(), bytes.size(), &view));
        return result(view);
    }

    explicit Loom(LoomSession *handle, std::string path, std::string passphrase,
                  std::vector<std::uint8_t> kek)
        : handle_(handle), path_(std::move(path)), passphrase_(std::move(passphrase)),
          kek_(std::move(kek)) {}
    LoomSession *handle_ = nullptr;
    std::string path_;
    std::string passphrase_;
    std::vector<std::uint8_t> kek_;
    std::string auth_principal_;
    std::string auth_passphrase_;
};

}  // namespace uldren::loom
