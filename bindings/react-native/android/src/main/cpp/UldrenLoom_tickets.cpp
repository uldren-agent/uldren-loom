#include "UldrenLoom_jni.h"

static jstring finishTicketsString(JNIEnv *env, LoomSession *h, int32_t st, char *out) {
  loom_close(h);
  if (st != 0) {
    throwLoom(env);
    return nullptr;
  }
  jstring result = env->NewStringUTF(out ? out : "");
  if (out) {
    loom_string_free(out);
  }
  return result;
}

#define TICKETS_OPEN() \
  const char *p = env->GetStringUTFChars(loomPath, nullptr); \
  LoomSession *h = nullptr; \
  int32_t st = openAuthenticatedStoreKeyed(env, p, passphrase, kek, authPrincipal, authPassphrase, &h); \
  env->ReleaseStringUTFChars(loomPath, p); \
  if (st != 0) { throwLoom(env); return nullptr; } \
  const char *n = env->GetStringUTFChars(ns, nullptr); \
  const char *tw = env->GetStringUTFChars(ticketWorkspaceId, nullptr)

#define TICKETS_RELEASE_NS() \
  env->ReleaseStringUTFChars(ns, n); \
  env->ReleaseStringUTFChars(ticketWorkspaceId, tw)

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsProjectCreateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jstring keyPrefix, jstring name, jstring expectedRoot,
    jbyteArray passphrase, jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  const char *prefix = env->GetStringUTFChars(keyPrefix, nullptr);
  const char *nm = env->GetStringUTFChars(name, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_project_create_json(h, n, tw, project, prefix, nm, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  env->ReleaseStringUTFChars(keyPrefix, prefix);
  env->ReleaseStringUTFChars(name, nm);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsProjectRekeyJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jstring keyPrefix, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  const char *prefix = env->GetStringUTFChars(keyPrefix, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_project_rekey_json(h, n, tw, project, prefix, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  env->ReleaseStringUTFChars(keyPrefix, prefix);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsProjectSettingsGetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  char *out = nullptr;
  st = loom_tickets_project_settings_get_json(h, n, tw, project, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsProjectSettingsSetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jstring defaultProjection, jstring enableProjectionsJson,
    jstring disableProjectionsJson, jstring actorEnforcement, jstring projectOwnerPrincipal,
    jboolean clearProjectOwnerPrincipal, jstring acceptanceAuthoritiesJson,
    jstring expectedRoot, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  const char *def = env->GetStringUTFChars(defaultProjection, nullptr);
  const char *enable = env->GetStringUTFChars(enableProjectionsJson, nullptr);
  const char *disable = env->GetStringUTFChars(disableProjectionsJson, nullptr);
  const char *actor = env->GetStringUTFChars(actorEnforcement, nullptr);
  const char *owner = env->GetStringUTFChars(projectOwnerPrincipal, nullptr);
  const char *authorities = env->GetStringUTFChars(acceptanceAuthoritiesJson, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  const char *defArg = def[0] != '\0' ? def : nullptr;
  const char *actorArg = actor[0] != '\0' ? actor : nullptr;
  const char *ownerArg = owner[0] != '\0' ? owner : nullptr;
  const char *authoritiesArg = authorities[0] != '\0' ? authorities : nullptr;
  char *out = nullptr;
  st = loom_tickets_project_settings_set_json(
      h, n, tw, project, defArg, enable, disable, actorArg, ownerArg,
      clearProjectOwnerPrincipal, authoritiesArg, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  env->ReleaseStringUTFChars(defaultProjection, def);
  env->ReleaseStringUTFChars(enableProjectionsJson, enable);
  env->ReleaseStringUTFChars(disableProjectionsJson, disable);
  env->ReleaseStringUTFChars(actorEnforcement, actor);
  env->ReleaseStringUTFChars(projectOwnerPrincipal, owner);
  env->ReleaseStringUTFChars(acceptanceAuthoritiesJson, authorities);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsFieldsJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jstring projection, jstring operation, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  const char *proj = env->GetStringUTFChars(projection, nullptr);
  const char *op = env->GetStringUTFChars(operation, nullptr);
  char *out = nullptr;
  st = loom_tickets_fields_json(h, n, tw, project, proj, op, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  env->ReleaseStringUTFChars(projection, proj);
  env->ReleaseStringUTFChars(operation, op);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsFieldPutJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jstring fieldId, jstring fieldKey, jstring name, jstring description,
    jstring fieldType, jstring optionSet, jdouble maxLength, jboolean hasMaxLength,
    jboolean required, jboolean searchable, jboolean orderable, jstring cardinality,
    jstring applicableTypeIdsJson, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  const char *field = env->GetStringUTFChars(fieldId, nullptr);
  const char *field_key = env->GetStringUTFChars(fieldKey, nullptr);
  const char *field_name = env->GetStringUTFChars(name, nullptr);
  const char *desc = env->GetStringUTFChars(description, nullptr);
  const char *typ = env->GetStringUTFChars(fieldType, nullptr);
  const char *options = env->GetStringUTFChars(optionSet, nullptr);
  const char *card = env->GetStringUTFChars(cardinality, nullptr);
  const char *applicable = env->GetStringUTFChars(applicableTypeIdsJson, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  const char *descArg = desc[0] != '\0' ? desc : nullptr;
  const char *optionsArg = options[0] != '\0' ? options : nullptr;
  char *out = nullptr;
  st = loom_tickets_field_put_json(
      h, n, tw, project, field, field_key, field_name, descArg, typ, optionsArg,
      static_cast<uint32_t>(maxLength), hasMaxLength, required, searchable, orderable,
      card, applicable, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  env->ReleaseStringUTFChars(fieldId, field);
  env->ReleaseStringUTFChars(fieldKey, field_key);
  env->ReleaseStringUTFChars(name, field_name);
  env->ReleaseStringUTFChars(description, desc);
  env->ReleaseStringUTFChars(fieldType, typ);
  env->ReleaseStringUTFChars(optionSet, options);
  env->ReleaseStringUTFChars(cardinality, card);
  env->ReleaseStringUTFChars(applicableTypeIdsJson, applicable);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsFieldRetireJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jstring fieldId, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  const char *field = env->GetStringUTFChars(fieldId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_field_retire_json(h, n, tw, project, field, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  env->ReleaseStringUTFChars(fieldId, field);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsCreateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projectId, jstring ticketType, jstring externalSource, jstring externalId,
    jstring fieldsJson, jstring policyLabelsJson, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *project = env->GetStringUTFChars(projectId, nullptr);
  const char *typ = env->GetStringUTFChars(ticketType, nullptr);
  const char *source = externalSource ? env->GetStringUTFChars(externalSource, nullptr) : nullptr;
  const char *external = externalId ? env->GetStringUTFChars(externalId, nullptr) : nullptr;
  const char *fields = env->GetStringUTFChars(fieldsJson, nullptr);
  const char *labels = env->GetStringUTFChars(policyLabelsJson, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_create_json(h, n, tw, project, typ, source, external, fields, labels, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projectId, project);
  env->ReleaseStringUTFChars(ticketType, typ);
  if (source) env->ReleaseStringUTFChars(externalSource, source);
  if (external) env->ReleaseStringUTFChars(externalId, external);
  env->ReleaseStringUTFChars(fieldsJson, fields);
  env->ReleaseStringUTFChars(policyLabelsJson, labels);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsUpdateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring setFieldsJson, jstring deleteFieldsJson, jstring action,
    jstring targetStatus, jstring observedSourceStatus, jstring observedWorkflowVersion,
    jstring assignee, jstring expectedRoot, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase, jstring commentId,
    jstring commentType, jstring commentBody, jstring commentsJson, jstring relationSetsJson,
    jstring relationRemovesJson) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *setFields = env->GetStringUTFChars(setFieldsJson, nullptr);
  const char *deleteFields = env->GetStringUTFChars(deleteFieldsJson, nullptr);
  const char *act = action ? env->GetStringUTFChars(action, nullptr) : nullptr;
  const char *status = targetStatus ? env->GetStringUTFChars(targetStatus, nullptr) : nullptr;
  const char *sourceStatus = observedSourceStatus ? env->GetStringUTFChars(observedSourceStatus, nullptr) : nullptr;
  const char *workflow = observedWorkflowVersion ? env->GetStringUTFChars(observedWorkflowVersion, nullptr) : nullptr;
  const char *assign = assignee ? env->GetStringUTFChars(assignee, nullptr) : nullptr;
  const char *comment = commentId ? env->GetStringUTFChars(commentId, nullptr) : nullptr;
  const char *commentKind = commentType ? env->GetStringUTFChars(commentType, nullptr) : nullptr;
  const char *body = commentBody ? env->GetStringUTFChars(commentBody, nullptr) : nullptr;
  const char *comments = commentsJson ? env->GetStringUTFChars(commentsJson, nullptr) : nullptr;
  const char *relationSets = relationSetsJson ? env->GetStringUTFChars(relationSetsJson, nullptr) : nullptr;
  const char *relationRemoves = relationRemovesJson ? env->GetStringUTFChars(relationRemovesJson, nullptr) : nullptr;
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_update_json(
      h, n, tw, ticket, setFields, deleteFields, act, status, sourceStatus, workflow, assign,
      comment, commentKind, body, root, comments, relationSets, relationRemoves, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(setFieldsJson, setFields);
  env->ReleaseStringUTFChars(deleteFieldsJson, deleteFields);
  if (act) env->ReleaseStringUTFChars(action, act);
  if (status) env->ReleaseStringUTFChars(targetStatus, status);
  if (sourceStatus) env->ReleaseStringUTFChars(observedSourceStatus, sourceStatus);
  if (workflow) env->ReleaseStringUTFChars(observedWorkflowVersion, workflow);
  if (assign) env->ReleaseStringUTFChars(assignee, assign);
  if (comment) env->ReleaseStringUTFChars(commentId, comment);
  if (commentKind) env->ReleaseStringUTFChars(commentType, commentKind);
  if (body) env->ReleaseStringUTFChars(commentBody, body);
  if (comments) env->ReleaseStringUTFChars(commentsJson, comments);
  if (relationSets) env->ReleaseStringUTFChars(relationSetsJson, relationSets);
  if (relationRemoves) env->ReleaseStringUTFChars(relationRemovesJson, relationRemoves);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsDeleteJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring expectedRoot, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_delete_json(h, n, tw, ticket, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsCommentsJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  char *out = nullptr;
  st = loom_tickets_comments_json(h, n, tw, ticket, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsCommentAddJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring commentId, jstring commentType, jstring body,
    jstring expectedRoot, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *comment = env->GetStringUTFChars(commentId, nullptr);
  const char *type = env->GetStringUTFChars(commentType, nullptr);
  const char *content = env->GetStringUTFChars(body, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_comment_add_json(h, n, tw, ticket, comment, type, content, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(commentId, comment);
  env->ReleaseStringUTFChars(commentType, type);
  env->ReleaseStringUTFChars(body, content);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsCommentUpdateJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring commentId, jstring commentType, jstring body,
    jstring expectedRoot, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *comment = env->GetStringUTFChars(commentId, nullptr);
  const char *type = env->GetStringUTFChars(commentType, nullptr);
  const char *content = env->GetStringUTFChars(body, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_comment_update_json(h, n, tw, ticket, comment, type, content, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(commentId, comment);
  env->ReleaseStringUTFChars(commentType, type);
  env->ReleaseStringUTFChars(body, content);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsCommentDeleteJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring commentId, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *comment = env->GetStringUTFChars(commentId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_comment_delete_json(h, n, tw, ticket, comment, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(commentId, comment);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsRelationSetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring relationId, jstring kind, jstring targetId,
    jstring expectedRoot, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *relation = env->GetStringUTFChars(relationId, nullptr);
  const char *kd = env->GetStringUTFChars(kind, nullptr);
  const char *target = env->GetStringUTFChars(targetId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_relation_set_json(h, n, tw, ticket, relation, kd, target, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(relationId, relation);
  env->ReleaseStringUTFChars(kind, kd);
  env->ReleaseStringUTFChars(targetId, target);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsRelationRemoveJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring relationId, jstring expectedRoot, jbyteArray passphrase,
    jbyteArray kek, jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *relation = env->GetStringUTFChars(relationId, nullptr);
  const char *root = env->GetStringUTFChars(expectedRoot, nullptr);
  char *out = nullptr;
  st = loom_tickets_relation_remove_json(h, n, tw, ticket, relation, root, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(relationId, relation);
  env->ReleaseStringUTFChars(expectedRoot, root);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsGetJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jstring projection, jbyteArray passphrase, jbyteArray kek,
    jstring authPrincipal, jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  const char *proj = env->GetStringUTFChars(projection, nullptr);
  char *out = nullptr;
  st = loom_tickets_get_json(h, n, tw, ticket, proj, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  env->ReleaseStringUTFChars(projection, proj);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsListJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring projection, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *proj = env->GetStringUTFChars(projection, nullptr);
  char *out = nullptr;
  st = loom_tickets_list_json(h, n, tw, proj, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(projection, proj);
  return finishTicketsString(env, h, st, out);
}

extern "C" JNIEXPORT jstring JNICALL
Java_ai_uldren_loom_rn_UldrenLoomNative_nativeTicketsHistoryJson(
    JNIEnv *env, jobject thiz, jstring loomPath, jstring ns, jstring ticketWorkspaceId,
    jstring ticketId, jbyteArray passphrase, jbyteArray kek, jstring authPrincipal,
    jbyteArray authPassphrase) {
  (void)thiz;
  TICKETS_OPEN();
  const char *ticket = env->GetStringUTFChars(ticketId, nullptr);
  char *out = nullptr;
  st = loom_tickets_history_json(h, n, tw, ticket, &out);
  TICKETS_RELEASE_NS();
  env->ReleaseStringUTFChars(ticketId, ticket);
  return finishTicketsString(env, h, st, out);
}

#undef TICKETS_OPEN
#undef TICKETS_RELEASE_NS
