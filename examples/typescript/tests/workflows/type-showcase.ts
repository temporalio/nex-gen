import { getUser, UserCapability } from "../../type-showcase/index.ts";

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
  return {
    deactivated: true,
    displayName: renamedUser.displayName,
    email: updatedUser.email,
    hasReadProfile: (user.profile.capabilities & UserCapability.ReadProfile) !== 0,
    notificationTag: user.profile.notificationTarget.tag,
    userId: user.userId,
  };
}
