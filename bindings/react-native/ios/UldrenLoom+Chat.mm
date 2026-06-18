#import "UldrenLoom+Internal.h"

@implementation UldrenLoom (Chat)

- (void)chatString:(NSString *)loomPath
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

- (void)chatCreateChannelJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId channelHandle:(NSString *)channelHandle name:(NSString *)name passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_create_channel_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, channelHandle.UTF8String, name.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatRenameChannelJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId selector:(NSString *)selector channelHandle:(NSString *)channelHandle passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_rename_channel_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, selector.UTF8String, channelHandle.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatListChannelsJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_list_channels_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatPostMessageJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId messageId:(NSString *)messageId threadId:(NSString *)threadId bodyText:(NSString *)bodyText passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *thread = threadId.length > 0 ? threadId.UTF8String : NULL;
    return loom_chat_post_message_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, messageId.UTF8String, thread, bodyText.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatEditMessageJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId messageId:(NSString *)messageId bodyText:(NSString *)bodyText passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_edit_message_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, messageId.UTF8String, bodyText.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatRedactMessageJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId messageId:(NSString *)messageId reason:(NSString *)reason passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *why = reason.length > 0 ? reason.UTF8String : NULL;
    return loom_chat_redact_message_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, messageId.UTF8String, why, out);
  } resolve:resolve reject:reject];
}

- (void)chatCreateThreadJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId threadId:(NSString *)threadId parentMessageId:(NSString *)parentMessageId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_create_thread_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, threadId.UTF8String, parentMessageId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatCreateTaskJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId taskId:(NSString *)taskId messageId:(NSString *)messageId title:(NSString *)title passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_create_task_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, taskId.UTF8String, messageId.UTF8String, title.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatClaimTaskJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId taskId:(NSString *)taskId claimId:(NSString *)claimId leaseToken:(NSString *)leaseToken passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *lease = leaseToken.length > 0 ? leaseToken.UTF8String : NULL;
    return loom_chat_claim_task_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, taskId.UTF8String, claimId.UTF8String, lease, out);
  } resolve:resolve reject:reject];
}

- (void)chatCompleteTaskJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId taskId:(NSString *)taskId claimId:(NSString *)claimId resultMessageId:(NSString *)resultMessageId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *result = resultMessageId.length > 0 ? resultMessageId.UTF8String : NULL;
    return loom_chat_complete_task_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, taskId.UTF8String, claimId.UTF8String, result, out);
  } resolve:resolve reject:reject];
}

- (void)chatInvokeAgentJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId invocationId:(NSString *)invocationId agentPrincipal:(NSString *)agentPrincipal sourceMessageIdsJson:(NSString *)sourceMessageIdsJson promptText:(NSString *)promptText passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_invoke_agent_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, invocationId.UTF8String, agentPrincipal.UTF8String, sourceMessageIdsJson.UTF8String, promptText.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatAgentReplyJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId invocationId:(NSString *)invocationId messageId:(NSString *)messageId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_agent_reply_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, invocationId.UTF8String, messageId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatRequestHandoffJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId handoffId:(NSString *)handoffId fromAgentPrincipal:(NSString *)fromAgentPrincipal toPrincipal:(NSString *)toPrincipal reason:(NSString *)reason passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    const char *to = toPrincipal.length > 0 ? toPrincipal.UTF8String : NULL;
    const char *why = reason.length > 0 ? reason.UTF8String : NULL;
    return loom_chat_request_handoff_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, handoffId.UTF8String, fromAgentPrincipal.UTF8String, to, why, out);
  } resolve:resolve reject:reject];
}

- (void)chatAddReactionJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId messageId:(NSString *)messageId kind:(NSString *)kind passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_add_reaction_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, messageId.UTF8String, kind.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatRemoveReactionJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId messageId:(NSString *)messageId kind:(NSString *)kind passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_remove_reaction_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, messageId.UTF8String, kind.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatEmojiListJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_emoji_list_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatEmojiRegisterJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId kind:(NSString *)kind passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_emoji_register_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, kind.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatEmojiUnregisterJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId kind:(NSString *)kind passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_emoji_unregister_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, kind.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatMessagesJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_messages_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatCursorJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_cursor_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, out);
  } resolve:resolve reject:reject];
}

- (void)chatUpdateCursorJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId nextSequence:(NSString *)nextSequence passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t next = 0;
  if (!loomResolveU64(nextSequence, &next, reject)) {
    return;
  }
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_update_cursor_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, next, out);
  } resolve:resolve reject:reject];
}

- (void)chatFetchEventsJson:(NSString *)loomPath workspace:(NSString *)workspace chatWorkspaceId:(NSString *)chatWorkspaceId channelId:(NSString *)channelId fromSequence:(NSString *)fromSequence max:(NSString *)max passphrase:(NSString *)passphrase kek:(NSArray *)kek authPrincipal:(NSString *)authPrincipal authPassphrase:(NSString *)authPassphrase resolve:(RCTPromiseResolveBlock)resolve reject:(RCTPromiseRejectBlock)reject {
  uint64_t from = 0;
  uint64_t limit = 0;
  if (!loomResolveU64(fromSequence, &from, reject) || !loomResolveU64(max, &limit, reject)) {
    return;
  }
  [self chatString:loomPath passphrase:passphrase kek:kek authPrincipal:authPrincipal authPassphrase:authPassphrase call:^int32_t(LoomSession *h, char **out) {
    return loom_chat_fetch_events_json(h, workspace.UTF8String, chatWorkspaceId.UTF8String, channelId.UTF8String, from, (uintptr_t)limit, out);
  } resolve:resolve reject:reject];
}

@end
