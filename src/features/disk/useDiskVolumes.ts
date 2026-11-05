import { listDiskVolumes, type DiskVolumeInfo } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Unidades do PC (total, usado, livre). Fonte do Disco e do "livre" do Dashboard (doc 14 Q4). */
export const diskVolumes = createAgentResource<DiskVolumeInfo[]>({
  name: "disk.volumes",
  fetch: listDiskVolumes,
});

export function useDiskVolumes(options?: { auto?: boolean }) {
  const resource = diskVolumes.use(options);
  return {
    volumes: resource.data ?? [],
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
  };
}
