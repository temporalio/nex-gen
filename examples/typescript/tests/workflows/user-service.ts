import { UserService } from "../../wit/user-service/index.ts";

export async function userServiceCaller(): Promise<{
  initialEmail: string;
  updatedEmail: string;
  userId: string;
}> {
  const service = new UserService("user-service");
  const user = await service.getUser({ userId: "user-123" });
  const updatedUser = await user.updateEmail("new@example.com");
  return {
    initialEmail: user.email,
    updatedEmail: updatedUser.email,
    userId: user.userId,
  };
}
