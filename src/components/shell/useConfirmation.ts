import { useCallback, useState } from "react";
import type { ConfirmRequest } from "./dialogs/ConfirmationDialog";

export type ConfirmationInput = Omit<ConfirmRequest, "id" | "resolve">;
export type RunConfirmed = (request: ConfirmationInput, action: () => Promise<unknown>) => Promise<unknown>;
export type RequestConfirmation = (request: ConfirmationInput) => Promise<boolean>;

/** A confirmação local que toda ação sensível pede antes de rodar. */
export function useConfirmation() {
  const [confirmRequest, setConfirmRequest] = useState<ConfirmRequest | null>(null);

  const requestConfirmation = useCallback<RequestConfirmation>((request) => {
    return new Promise<boolean>((resolve) => {
      const id = crypto.randomUUID();
      let settled = false;
      const timeoutId = request.timeoutMs
        ? window.setTimeout(() => {
            if (settled) return;
            settled = true;
            setConfirmRequest((current) => (current?.id === id ? null : current));
            resolve(false);
          }, request.timeoutMs)
        : undefined;

      const nextRequest: ConfirmRequest = {
        ...request,
        id,
        resolve: (approved) => {
          if (settled) return;
          settled = true;
          if (timeoutId) window.clearTimeout(timeoutId);
          resolve(approved);
        },
      };
      setConfirmRequest((current) => {
        current?.resolve(false);
        return nextRequest;
      });
    });
  }, []);

  const closeConfirmation = useCallback((approved: boolean) => {
    setConfirmRequest((current) => {
      current?.resolve(approved);
      return null;
    });
  }, []);

  const runConfirmed = useCallback<RunConfirmed>(
    async (request, action) => {
      const approved = await requestConfirmation(request);
      if (!approved) return false;
      // Returns whatever the action resolves to (not just a boolean) so a
      // caller that needs the result - e.g. LocalControls reading the
      // scanId out of START_SYSTEM_FILE_CHECK's OptimizationResult to poll
      // it - can. Every existing caller already discards this return value
      // (`await runConfirmed(...)` as a bare statement), so widening it is
      // safe.
      return await action();
    },
    [requestConfirmation],
  );

  return { confirmRequest, requestConfirmation, closeConfirmation, runConfirmed };
}
