#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Tickets)

- (void)ticketsString:(NSString *)loomPath
           passphrase:(NSString *)passphrase
                  kek:(NSArray *)kek
        authPrincipal:(NSString *)authPrincipal
       authPassphrase:(NSString *)authPassphrase
                 call:(int32_t (^)(LoomSession *, char **))call
              resolve:(RCTPromiseResolveBlock)resolve
               reject:(RCTPromiseRejectBlock)reject {
  dispatch_async([self workQueue], ^{
    LoomSession *h = [self openStore:loomPath passphrase:passphrase kek:kek];
    if (h == NULL) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    char *out = NULL;
    int32_t st = [self authenticateStore:h principal:authPrincipal passphrase:authPassphrase];
    if (st == 0) {
      st = call(h, &out);
    }
    loom_close(h);
    if (st != 0) {
      NSError *err = [self loomError];
      reject([@(err.code) stringValue], err.localizedDescription, err);
      return;
    }
    NSString *result = out ? [NSString stringWithUTF8String:out] : @"";
    if (out) {
      loom_string_free(out);
    }
    resolve(result);
  });
}

- (void)ticketsProjectCreateJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId keyPrefix:(NSString *)keyPrefix name:(NSString *)name expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_project_create_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, keyPrefix.UTF8String, name.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsProjectRekeyJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId keyPrefix:(NSString *)keyPrefix expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_project_rekey_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, keyPrefix.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsProjectSettingsGetJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_project_settings_get_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsProjectSettingsSetJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId defaultProjection:(NSString *)defaultProjection enableProjectionsJson:(NSString *)enableProjectionsJson disableProjectionsJson:(NSString *)disableProjectionsJson actorEnforcement:(NSString *)actorEnforcement projectOwnerPrincipal:(NSString *)projectOwnerPrincipal clearProjectOwnerPrincipal:(BOOL)clearProjectOwnerPrincipal acceptanceAuthoritiesJson:(NSString *)acceptanceAuthoritiesJson expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *default_projection = defaultProjection.length > 0 ? defaultProjection.UTF8String : NULL;
    const char *actor = actorEnforcement.length > 0 ? actorEnforcement.UTF8String : NULL;
    const char *owner = projectOwnerPrincipal.length > 0 ? projectOwnerPrincipal.UTF8String : NULL;
    const char *authorities = acceptanceAuthoritiesJson.length > 0 ? acceptanceAuthoritiesJson.UTF8String : NULL;
    return loom_tickets_project_settings_set_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, default_projection, enableProjectionsJson.UTF8String, disableProjectionsJson.UTF8String, actor, owner, clearProjectOwnerPrincipal, authorities, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsFieldsJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId projection:(NSString *)projection operation:(NSString *)operation passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_fields_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, projection.UTF8String, operation.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsFieldPutJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId fieldId:(NSString *)fieldId fieldKey:(NSString *)fieldKey name:(NSString *)name description:(NSString *)description fieldType:(NSString *)fieldType optionSet:(NSString *)optionSet maxLength:(double)maxLength hasMaxLength:(BOOL)hasMaxLength required:(BOOL)required searchable:(BOOL)searchable orderable:(BOOL)orderable cardinality:(NSString *)cardinality applicableTypeIdsJson:(NSString *)applicableTypeIdsJson expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *desc = description.length > 0 ? description.UTF8String : NULL;
    const char *options = optionSet.length > 0 ? optionSet.UTF8String : NULL;
    return loom_tickets_field_put_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, fieldId.UTF8String, fieldKey.UTF8String, name.UTF8String, desc, fieldType.UTF8String, options, (uint32_t)maxLength, hasMaxLength, required, searchable, orderable, cardinality.UTF8String, applicableTypeIdsJson.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsFieldRetireJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId fieldId:(NSString *)fieldId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_field_retire_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, fieldId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsCreateJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projectId:(NSString *)projectId ticketType:(NSString *)ticketType externalSource:(NSString *)externalSource externalId:(NSString *)externalId fieldsJson:(NSString *)fieldsJson policyLabelsJson:(NSString *)policyLabelsJson expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *source = externalSource.length > 0 ? externalSource.UTF8String : NULL;
    const char *external = externalId.length > 0 ? externalId.UTF8String : NULL;
    return loom_tickets_create_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projectId.UTF8String, ticketType.UTF8String, source, external, fieldsJson.UTF8String, policyLabelsJson.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsUpdateJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId setFieldsJson:(NSString *)setFieldsJson deleteFieldsJson:(NSString *)deleteFieldsJson action:(NSString *)action targetStatus:(NSString *)targetStatus observedSourceStatus:(NSString *)observedSourceStatus observedWorkflowVersion:(NSString *)observedWorkflowVersion assignee:(NSString *)assignee expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase commentId:(NSString *)commentId commentType:(NSString *)commentType commentBody:(NSString *)commentBody commentsJson:(NSString *)commentsJson relationSetsJson:(NSString *)relationSetsJson relationRemovesJson:(NSString *)relationRemovesJson resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *act = action.length > 0 ? action.UTF8String : NULL;
    const char *status = targetStatus.length > 0 ? targetStatus.UTF8String : NULL;
    const char *sourceStatus = observedSourceStatus.length > 0 ? observedSourceStatus.UTF8String : NULL;
    const char *workflow = observedWorkflowVersion.length > 0 ? observedWorkflowVersion.UTF8String : NULL;
    const char *assigned = assignee.length > 0 ? assignee.UTF8String : NULL;
    const char *comment = commentId.length > 0 ? commentId.UTF8String : NULL;
    const char *commentKind = commentType.length > 0 ? commentType.UTF8String : NULL;
    const char *body = commentBody.length > 0 ? commentBody.UTF8String : NULL;
    const char *comments = commentsJson.length > 0 ? commentsJson.UTF8String : NULL;
    const char *relationSets = relationSetsJson.length > 0 ? relationSetsJson.UTF8String : NULL;
    const char *relationRemoves = relationRemovesJson.length > 0 ? relationRemovesJson.UTF8String : NULL;
    return loom_tickets_update_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, setFieldsJson.UTF8String, deleteFieldsJson.UTF8String, act, status, sourceStatus, workflow, assigned, comment, commentKind, body, expectedRoot.UTF8String, comments, relationSets, relationRemoves, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsDeleteJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_delete_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsCommentsJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_comments_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsCommentAddJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId commentId:(NSString *)commentId commentType:(NSString *)commentType body:(NSString *)body expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_comment_add_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, commentId.UTF8String, commentType.UTF8String, body.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsCommentUpdateJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId commentId:(NSString *)commentId commentType:(NSString *)commentType body:(NSString *)body expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_comment_update_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, commentId.UTF8String, commentType.UTF8String, body.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsCommentDeleteJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId commentId:(NSString *)commentId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_comment_delete_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, commentId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsRelationSetJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId relationId:(NSString *)relationId kind:(NSString *)kind targetId:(NSString *)targetId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_relation_set_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, relationId.UTF8String, kind.UTF8String, targetId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsRelationRemoveJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId relationId:(NSString *)relationId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_relation_remove_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, relationId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsGetJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId projection:(NSString *)projection passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_get_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, projection.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsListJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId projection:(NSString *)projection passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_list_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, projection.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)ticketsHistoryJson:(NSString *)loomPath workspace:(NSString *)workspace ticketWorkspaceId:(NSString *)ticketWorkspaceId ticketId:(NSString *)ticketId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self ticketsString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_tickets_history_json(h, workspace.UTF8String, ticketWorkspaceId.UTF8String, ticketId.UTF8String, out);
  } resolve:resolve reject:reject];
}

@end
