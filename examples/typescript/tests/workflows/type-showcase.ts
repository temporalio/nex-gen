import { getUser, recordSync } from "../../type-showcase/index.ts";
import { UserCapability, type SyncReport } from "../../type-showcase/models.ts";

const syncReport = (): SyncReport => ({
  route: [
    [45.5152, -122.6784],
    [47.6062, -122.3321],
  ],
  attempts: [
    { tag: "ok" as const, value: "synced" },
    { tag: "err" as const, value: "timeout" },
  ],
  // Dashed map keys exercise the type-directed runtime: map keys are data
  // and must be preserved verbatim, unlike record field names.
  regionStatus: {
    "us-west": { tag: "ok" as const, value: "healthy" },
    "eu-central": { tag: "err" as const, value: "degraded" },
  },
});

export async function typeShowcaseCaller(): Promise<{
  deactivated: boolean;
  displayName: string;
  email: string;
  hasReadProfile: boolean;
  notificationTag: string;
  userId: string;
}> {
  const user = await getUser({
    consistencyToken: "read-123",
    userId: "user-123",
  });
  const updatedUser = await user.updateEmail("new@example.com");
  const renamedUser = await updatedUser.rename("New Name");
  await renamedUser.deactivate("requested");
  const recordSyncHandle = await recordSync({
    userId: "user-123",
    report: syncReport(),
  });
  await recordSyncHandle.result();
  return {
    deactivated: true,
    displayName: renamedUser.displayName,
    email: updatedUser.email,
    hasReadProfile: (user.profile.capabilities & UserCapability.ReadProfile) !== 0,
    notificationTag: user.profile.notificationTarget.tag,
    userId: user.userId,
  };
}
