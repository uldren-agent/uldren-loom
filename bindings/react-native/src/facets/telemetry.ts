import UldrenLoom from '../NativeUldrenLoom';
import { authArgs, keyArgs } from '../internal';
import type { LoomAuth, LoomKey } from '../internal';

export function metricsPutDescriptor(
  loomPath: string,
  workspace: string,
  descriptor: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.metricsPutDescriptor(
    loomPath,
    workspace,
    Array.from(descriptor),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export async function metricsGetDescriptor(
  loomPath: string,
  workspace: string,
  name: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.metricsGetDescriptor(
    loomPath,
    workspace,
    name,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

export function metricsPutObservation(
  loomPath: string,
  workspace: string,
  descriptorName: string,
  observation: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.metricsPutObservation(
    loomPath,
    workspace,
    descriptorName,
    Array.from(observation),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export async function metricsQuery(
  loomPath: string,
  workspace: string,
  descriptorName: string,
  fromTimestampMs: number | bigint | string,
  toTimestampMs: number | bigint | string,
  maxSeries: number,
  maxGroups: number,
  maxSamples: number,
  maxOutputBytes: number | bigint | string,
  nowTimestampMs: number | bigint | string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.metricsQuery(
    loomPath,
    workspace,
    descriptorName,
    String(fromTimestampMs),
    String(toTimestampMs),
    maxSeries,
    maxGroups,
    maxSamples,
    String(maxOutputBytes),
    String(nowTimestampMs),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export function logsPutRecord(
  loomPath: string,
  workspace: string,
  record: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<string> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.logsPutRecord(
    loomPath,
    workspace,
    Array.from(record),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export async function logsGetRecord(
  loomPath: string,
  workspace: string,
  recordId: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.logsGetRecord(
    loomPath,
    workspace,
    recordId,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

export async function logsQuery(
  loomPath: string,
  workspace: string,
  fromTimeUnixNano: number | bigint | string,
  toTimeUnixNano: number | bigint | string,
  maxRecords: number,
  maxOutputBytes: number | bigint | string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.logsQuery(
    loomPath,
    workspace,
    String(fromTimeUnixNano),
    String(toTimeUnixNano),
    maxRecords,
    String(maxOutputBytes),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export function tracesPutSpan(
  loomPath: string,
  workspace: string,
  span: Uint8Array | number[],
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<void> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  return UldrenLoom.tracesPutSpan(
    loomPath,
    workspace,
    Array.from(span),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
}

export async function tracesGetSpan(
  loomPath: string,
  workspace: string,
  traceId: string,
  spanId: string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array | null> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.tracesGetSpan(
    loomPath,
    workspace,
    traceId,
    spanId,
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return bytes == null ? null : Uint8Array.from(bytes);
}

export async function tracesTraceSpans(
  loomPath: string,
  workspace: string,
  traceId: string,
  maxSpans: number,
  maxOutputBytes: number | bigint | string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.tracesTraceSpans(
    loomPath,
    workspace,
    traceId,
    maxSpans,
    String(maxOutputBytes),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}

export async function tracesQuery(
  loomPath: string,
  workspace: string,
  fromStartTimeNs: number | bigint | string,
  toStartTimeNs: number | bigint | string,
  maxSpans: number,
  maxOutputBytes: number | bigint | string,
  loomKey?: LoomKey,
  auth?: LoomAuth
): Promise<Uint8Array> {
  const [passphrase, kek] = keyArgs(loomKey);
  const [authPrincipal, authPassphrase] = authArgs(auth);
  const bytes = await UldrenLoom.tracesQuery(
    loomPath,
    workspace,
    String(fromStartTimeNs),
    String(toStartTimeNs),
    maxSpans,
    String(maxOutputBytes),
    passphrase,
    kek,
    authPrincipal,
    authPassphrase
  );
  return Uint8Array.from(bytes);
}
