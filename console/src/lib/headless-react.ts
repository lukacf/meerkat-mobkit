import * as React from "react";

import {
  createMobKitConsoleController,
  type MobKitConsoleController,
  type MobKitConsoleTransport,
} from "./headless";

export function useMobKitConsoleController({
  transport,
}: {
  transport: MobKitConsoleTransport;
}): MobKitConsoleController {
  return React.useMemo(
    () => createMobKitConsoleController({ transport }),
    [transport],
  );
}
