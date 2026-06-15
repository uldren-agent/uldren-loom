import type { TurboModule } from 'react-native';
import { TurboModuleRegistry } from 'react-native';

// Codegen spec (new architecture). `bytes` is an array of 0-255 byte values; the result is the
// content address "algo:hex".
export interface Spec extends TurboModule {
  version(): string;
  blobDigest(bytes: number[]): string;
}

export default TurboModuleRegistry.getEnforcing<Spec>('UldrenLoom');
