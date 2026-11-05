import { scanCleanupCategories, type CleanupCategory } from "@/services/tauri/agent";
import { createAgentResource } from "@/features/shared/agentResource";

/** Categorias de limpeza e quanto cada uma libera. Fonte única (doc 14 Q2). */
export const cleanupCategories = createAgentResource<CleanupCategory[]>({
  name: "cleanup.categories",
  fetch: scanCleanupCategories,
});

export function useCleanup(options?: { auto?: boolean }) {
  const resource = cleanupCategories.use(options);
  return {
    categories: resource.data ?? [],
    loading: resource.loading,
    error: resource.error,
    refresh: resource.refresh,
    set: resource.set,
  };
}
