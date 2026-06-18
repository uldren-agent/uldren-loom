import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

function args(key?: LoomKey, auth?: LoomAuth): [string, number[], string, string] {
  const [passphrase, kek] = keyArgs(key);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return [passphrase, kek, authPrincipal, authPassphrase];
}

function bytes(value: Uint8Array | number[]): number[] {
  return Array.from(value);
}

function out(value: number[] | null): Uint8Array | null {
  return value == null ? null : Uint8Array.from(value);
}

export function lanesCreate(loomPath: string, workspace: string, lane: Uint8Array | number[], key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  return UldrenLoom.lanesCreate(loomPath, workspace, bytes(lane), ...args(key, auth)).then(Uint8Array.from);
}

export function lanesGet(loomPath: string, workspace: string, laneId: string, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array | null> {
  return UldrenLoom.lanesGet(loomPath, workspace, laneId, ...args(key, auth)).then(out);
}

export function lanesList(loomPath: string, workspace: string, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  return UldrenLoom.lanesList(loomPath, workspace, ...args(key, auth)).then(Uint8Array.from);
}

export function lanesUpdate(loomPath: string, workspace: string, laneId: string, fields: { title?: string | null; description?: string | null; laneStatus?: string | null; statusReport?: string | null; reviewerFeedback?: string | null }, updatedBy: string, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  return UldrenLoom.lanesUpdate(loomPath, workspace, laneId, fields.title ?? null, fields.description ?? null, fields.laneStatus ?? null, fields.statusReport ?? null, fields.reviewerFeedback ?? null, updatedBy, ...args(key, auth)).then(Uint8Array.from);
}

export function lanesTicketAdd(loomPath: string, workspace: string, laneId: string, ticketId: string, updatedBy: string, placement: string = 'append', anchor?: string | null, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  return UldrenLoom.lanesTicketAdd(loomPath, workspace, laneId, ticketId, updatedBy, placement, anchor ?? '', ...args(key, auth)).then(Uint8Array.from);
}

export function lanesTicketRemove(loomPath: string, workspace: string, laneId: string, ticketId: string, updatedBy: string, key?: LoomKey, auth?: LoomAuth): Promise<Uint8Array> {
  return UldrenLoom.lanesTicketRemove(loomPath, workspace, laneId, ticketId, updatedBy, ...args(key, auth)).then(Uint8Array.from);
}
