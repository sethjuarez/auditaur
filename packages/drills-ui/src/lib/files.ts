import { withToken } from "./api";

// A panel-server URL for a file inside a drill run's folder.
export const fileUrl = (runId: string, path: string) =>
  withToken(`/api/file?runId=${encodeURIComponent(runId)}&path=${encodeURIComponent(path)}`);