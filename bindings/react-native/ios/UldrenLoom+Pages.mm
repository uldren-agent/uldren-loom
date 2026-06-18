#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Pages)

- (void)pagesString:(NSString *)loomPath passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase call:(int32_t (^)(LoomSession *, char **))call resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
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

- (void)spacesCreateJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId spaceId:(NSString *)spaceId title:(NSString *)title expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_spaces_create_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, spaceId.UTF8String, title.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)spacesListJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_spaces_list_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)spacesGetJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId spaceId:(NSString *)spaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_spaces_get_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, spaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)pagesCreateJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId pageId:(NSString *)pageId spaceId:(NSString *)spaceId parentPageId:(NSString *)parentPageId title:(NSString *)title expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_pages_create_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, pageId.UTF8String, spaceId.UTF8String, parentPageId.UTF8String, title.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)pagesUpdateJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId pageId:(NSString *)pageId bodyText:(NSString *)bodyText expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_pages_update_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, pageId.UTF8String, bodyText.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)pagesPublishJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId pageId:(NSString *)pageId expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_pages_publish_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, pageId.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

#define PAGE_GETTER(objc_name, c_name, id_name) \
- (void)objc_name:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId id_name:(NSString *)id_name passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject { \
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) { \
    return c_name(h, workspace.UTF8String, pageWorkspaceId.UTF8String, id_name.UTF8String, out); \
  } resolve:resolve reject:reject]; \
}

PAGE_GETTER(pagesGetJson, loom_pages_get_json, pageId)
PAGE_GETTER(pagesHistoryJson, loom_pages_history_json, pageId)
PAGE_GETTER(structuresGetJson, loom_structures_get_json, structureId)

- (void)pagesListJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_pages_list_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)structuresCreateJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId structureId:(NSString *)structureId spaceId:(NSString *)spaceId kind:(NSString *)kind title:(NSString *)title expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_structures_create_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, structureId.UTF8String, spaceId.UTF8String, kind.UTF8String, title.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

#define STRUCTURE_NODE(objc_name, c_name) \
- (void)objc_name:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId structureId:(NSString *)structureId nodeId:(NSString *)nodeId kind:(NSString *)kind label:(NSString *)label bodyDigest:(NSString *)bodyDigest entityRef:(NSString *)entityRef expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject { \
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) { \
    return c_name(h, workspace.UTF8String, pageWorkspaceId.UTF8String, structureId.UTF8String, nodeId.UTF8String, kind.UTF8String, label.UTF8String, bodyDigest.UTF8String, entityRef.UTF8String, expectedRoot.UTF8String, out); \
  } resolve:resolve reject:reject]; \
}

STRUCTURE_NODE(structuresAddNodeJson, loom_structures_add_node_json)
STRUCTURE_NODE(structuresUpdateNodeJson, loom_structures_update_node_json)

- (void)structuresBindJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId structureId:(NSString *)structureId nodeId:(NSString *)nodeId entityRef:(NSString *)entityRef expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_structures_bind_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, structureId.UTF8String, nodeId.UTF8String, entityRef.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)structuresMoveNodeJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId structureId:(NSString *)structureId nodeId:(NSString *)nodeId parentNodeId:(NSString *)parentNodeId label:(NSString *)label expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_structures_move_node_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, structureId.UTF8String, nodeId.UTF8String, parentNodeId.UTF8String, label.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)structuresLinkNodeJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId structureId:(NSString *)structureId edgeId:(NSString *)edgeId srcNodeId:(NSString *)srcNodeId dstNodeId:(NSString *)dstNodeId label:(NSString *)label targetRef:(NSString *)targetRef expectedRoot:(NSString *)expectedRoot passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_structures_link_node_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, structureId.UTF8String, edgeId.UTF8String, srcNodeId.UTF8String, dstNodeId.UTF8String, label.UTF8String, targetRef.UTF8String, expectedRoot.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)structuresDecomposeToTicketsJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId structureId:(NSString *)structureId itemsJson:(NSString *)itemsJson passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_structures_decompose_to_tickets_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, structureId.UTF8String, itemsJson.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)structuresListJson:(NSString *)loomPath workspace:(NSString *)workspace pageWorkspaceId:(NSString *)pageWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self pagesString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_structures_list_json(h, workspace.UTF8String, pageWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

@end
