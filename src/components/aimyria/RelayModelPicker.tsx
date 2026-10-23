/**
 * Model picker for the two clients that write a flat model list.
 *
 * # Why this is a dialog and not a field on the row
 *
 * WorkBuddy and DeepSeek Harness receive *several* model records from one
 * endpoint. Letting the user choose which ones is the difference between "接入"
 * writing all sixteen models the relay sells into their app's model menu and
 * writing the two they actually use.
 *
 * # Where the list comes from
 *
 * The gateway, via `get_available_models`. That answers "what may this key call",
 * which is the only list that matters — a catalogue of everything the relay sells
 * would offer models the key cannot use, and the failure would appear at request
 * time.
 *
 * Manual entry stays available for a model the gateway does not list yet.
 */

import { useEffect, useMemo, useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Check, Loader2, Plus, X } from "lucide-react";

import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Input } from "@/components/ui/input";
import { getAvailableModels } from "@/lib/api/aimyria";
import { cn } from "@/lib/utils";

export interface ModelPickerProps {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  /** Which client this is for, shown in the title. */
  appLabel: string;
  /** Models to pre-select. */
  initial?: string[];
  /** Called with the chosen list. Empty is rejected before this runs. */
  onConfirm: (models: string[]) => void;
  busy?: boolean;
}

export function RelayModelPicker({
  open,
  onOpenChange,
  appLabel,
  initial = [],
  onConfirm,
  busy = false,
}: ModelPickerProps) {
  const [selected, setSelected] = useState<string[]>(initial);
  const [manual, setManual] = useState("");
  const [error, setError] = useState<string | null>(null);

  const modelsQuery = useQuery({
    queryKey: ["relay-available-models"],
    queryFn: () => getAvailableModels(),
    enabled: open,
    staleTime: 5 * 60_000,
  });

  // Reset when reopened so a cancelled pick does not leak into the next one.
  useEffect(() => {
    if (open) {
      setSelected(initial);
      setManual("");
      setError(null);
    }
    // `initial` is a fresh array on each render of the parent; keying on the open
    // transition is what we actually want.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [open]);

  const available = useMemo(() => modelsQuery.data ?? [], [modelsQuery.data]);

  const toggle = (id: string) => {
    setSelected((current) =>
      current.includes(id)
        ? current.filter((item) => item !== id)
        : [...current, id],
    );
    setError(null);
  };

  const addManual = () => {
    const trimmed = manual.trim();
    if (!trimmed) return;
    if (!selected.includes(trimmed)) {
      setSelected((current) => [...current, trimmed]);
    }
    setManual("");
    setError(null);
  };

  const submit = () => {
    if (selected.length === 0) {
      // Writing an empty list would produce a client with no model menu, which is
      // worse than not writing at all.
      setError("至少选择一个模型");
      return;
    }
    onConfirm(selected);
  };

  return (
    <Dialog open={open} onOpenChange={(next) => !busy && onOpenChange(next)}>
      <DialogContent className="max-w-[440px]">
        <DialogHeader className="space-y-2 border-b-0 bg-transparent pb-0">
          <DialogTitle className="text-base font-semibold">
            选择要写入 {appLabel} 的模型
          </DialogTitle>
          <DialogDescription className="text-sm leading-6">
            只写入你选中的模型。之后可以随时改选。
          </DialogDescription>
        </DialogHeader>

        <div className="px-6 pt-4">
          {modelsQuery.isLoading ? (
            <p className="py-8 text-center text-caption text-fg-3">
              <Loader2 className="mr-1.5 inline h-3.5 w-3.5 animate-spin" />
              正在从网关读取…
            </p>
          ) : modelsQuery.isError ? (
            <div className="py-4">
              <p className="text-caption text-danger-text">读不到模型列表</p>
              <p className="mt-1 text-[11px] leading-5 text-fg-3">
                可以直接在下面手动添加模型 ID。
              </p>
            </div>
          ) : available.length === 0 ? (
            <p className="py-6 text-center text-caption text-fg-3">
              这个密钥还没有可用模型
            </p>
          ) : (
            <div className="max-h-[260px] overflow-y-auto rounded-lg border border-border">
              {available.map((id) => {
                const checked = selected.includes(id);
                return (
                  <button
                    key={id}
                    type="button"
                    disabled={busy}
                    onClick={() => toggle(id)}
                    className="flex w-full items-center gap-2.5 px-3 py-2 text-left transition-colors hover:bg-subtle disabled:opacity-50"
                  >
                    <span
                      className={cn(
                        "flex h-4 w-4 shrink-0 items-center justify-center rounded border",
                        checked
                          ? "border-action bg-action text-action-fg"
                          : "border-border-strong",
                      )}
                      aria-hidden
                    >
                      {checked ? <Check className="h-3 w-3" /> : null}
                    </span>
                    <span className="truncate text-sm text-fg-1">{id}</span>
                  </button>
                );
              })}
            </div>
          )}

          <div className="mt-3 flex gap-2">
            <Input
              value={manual}
              disabled={busy}
              placeholder="手动添加模型 ID"
              onChange={(event) => setManual(event.target.value)}
              onKeyDown={(event) => {
                if (event.key === "Enter") {
                  event.preventDefault();
                  addManual();
                }
              }}
            />
            <Button
              variant="outline"
              size="compact"
              disabled={busy || !manual.trim()}
              onClick={addManual}
            >
              <Plus className="h-3.5 w-3.5" />
            </Button>
          </div>

          {selected.length > 0 ? (
            <div className="mt-3 flex flex-wrap gap-1.5">
              {selected.map((id) => (
                <span
                  key={id}
                  className="flex items-center gap-1 rounded-full border border-border px-2 py-0.5 text-[11px] text-fg-2"
                >
                  {id}
                  <button
                    type="button"
                    aria-label={`移除 ${id}`}
                    disabled={busy}
                    onClick={() => toggle(id)}
                    className="text-fg-3 transition-colors hover:text-fg-1"
                  >
                    <X className="h-3 w-3" />
                  </button>
                </span>
              ))}
            </div>
          ) : null}

          {error ? (
            <p className="mt-2 text-caption text-danger-text">{error}</p>
          ) : null}
        </div>

        <DialogFooter className="flex items-center justify-between border-t-0 bg-transparent pt-3">
          <span className="text-caption text-fg-3">已选 {selected.length}</span>
          <div className="flex gap-2">
            <Button
              variant="outline"
              disabled={busy}
              onClick={() => onOpenChange(false)}
            >
              取消
            </Button>
            <Button disabled={busy} onClick={submit}>
              {busy ? <Loader2 className="mr-1.5 h-3.5 w-3.5 animate-spin" /> : null}
              写入
            </Button>
          </div>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}
