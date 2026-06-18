import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

function args(key?: LoomKey, auth?: LoomAuth): [string, number[], string, string] {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return [passphrase, kek, authPrincipal, authPassphrase];
}

export function chatCreateChannelJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, channelHandle: string, name: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatCreateChannelJson(loomPath, workspace, chatWorkspaceId, channelId, channelHandle, name, ...args(key, auth));
}

export function chatRenameChannelJson(loomPath: string, workspace: string, chatWorkspaceId: string, selector: string, channelHandle: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatRenameChannelJson(loomPath, workspace, chatWorkspaceId, selector, channelHandle, ...args(key, auth));
}

export function chatListChannelsJson(loomPath: string, workspace: string, chatWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatListChannelsJson(loomPath, workspace, chatWorkspaceId, ...args(key, auth));
}

export function chatPostMessageJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, threadId: string | null | undefined, bodyText: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatPostMessageJson(loomPath, workspace, chatWorkspaceId, channelId, messageId, threadId ?? '', bodyText, ...args(key, auth));
}

export function chatEditMessageJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, bodyText: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatEditMessageJson(loomPath, workspace, chatWorkspaceId, channelId, messageId, bodyText, ...args(key, auth));
}

export function chatRedactMessageJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, reason: string | null | undefined, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatRedactMessageJson(loomPath, workspace, chatWorkspaceId, channelId, messageId, reason ?? '', ...args(key, auth));
}

export function chatCreateThreadJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, threadId: string, parentMessageId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatCreateThreadJson(loomPath, workspace, chatWorkspaceId, channelId, threadId, parentMessageId, ...args(key, auth));
}

export function chatCreateTaskJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, messageId: string, title: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatCreateTaskJson(loomPath, workspace, chatWorkspaceId, channelId, taskId, messageId, title, ...args(key, auth));
}

export function chatClaimTaskJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, claimId: string, leaseToken: string | null | undefined, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatClaimTaskJson(loomPath, workspace, chatWorkspaceId, channelId, taskId, claimId, leaseToken ?? '', ...args(key, auth));
}

export function chatCompleteTaskJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, taskId: string, claimId: string, resultMessageId: string | null | undefined, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatCompleteTaskJson(loomPath, workspace, chatWorkspaceId, channelId, taskId, claimId, resultMessageId ?? '', ...args(key, auth));
}

export function chatInvokeAgentJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, invocationId: string, agentPrincipal: string, sourceMessageIdsJson: string, promptText: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatInvokeAgentJson(loomPath, workspace, chatWorkspaceId, channelId, invocationId, agentPrincipal, sourceMessageIdsJson, promptText, ...args(key, auth));
}

export function chatAgentReplyJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, invocationId: string, messageId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatAgentReplyJson(loomPath, workspace, chatWorkspaceId, channelId, invocationId, messageId, ...args(key, auth));
}

export function chatRequestHandoffJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, handoffId: string, fromAgentPrincipal: string, toPrincipal: string | null | undefined, reason: string | null | undefined, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatRequestHandoffJson(loomPath, workspace, chatWorkspaceId, channelId, handoffId, fromAgentPrincipal, toPrincipal ?? '', reason ?? '', ...args(key, auth));
}

export function chatAddReactionJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatAddReactionJson(loomPath, workspace, chatWorkspaceId, channelId, messageId, kind, ...args(key, auth));
}

export function chatRemoveReactionJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, messageId: string, kind: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatRemoveReactionJson(loomPath, workspace, chatWorkspaceId, channelId, messageId, kind, ...args(key, auth));
}

export function chatEmojiListJson(loomPath: string, workspace: string, chatWorkspaceId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatEmojiListJson(loomPath, workspace, chatWorkspaceId, ...args(key, auth));
}

export function chatEmojiRegisterJson(loomPath: string, workspace: string, chatWorkspaceId: string, kind: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatEmojiRegisterJson(loomPath, workspace, chatWorkspaceId, kind, ...args(key, auth));
}

export function chatEmojiUnregisterJson(loomPath: string, workspace: string, chatWorkspaceId: string, kind: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatEmojiUnregisterJson(loomPath, workspace, chatWorkspaceId, kind, ...args(key, auth));
}

export function chatMessagesJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatMessagesJson(loomPath, workspace, chatWorkspaceId, channelId, ...args(key, auth));
}

export function chatCursorJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatCursorJson(loomPath, workspace, chatWorkspaceId, channelId, ...args(key, auth));
}

export function chatUpdateCursorJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, nextSequence: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatUpdateCursorJson(loomPath, workspace, chatWorkspaceId, channelId, nextSequence, ...args(key, auth));
}

export function chatFetchEventsJson(loomPath: string, workspace: string, chatWorkspaceId: string, channelId: string, fromSequence: string, max: string, key?: LoomKey, auth?: LoomAuth): Promise<string> {
  return UldrenLoom.chatFetchEventsJson(loomPath, workspace, chatWorkspaceId, channelId, fromSequence, max, ...args(key, auth));
}
