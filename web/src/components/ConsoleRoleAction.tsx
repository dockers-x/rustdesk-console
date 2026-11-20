import { useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Button } from "@cloudflare/kumo/components/button";
import { Checkbox } from "@cloudflare/kumo/components/checkbox";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Select } from "@cloudflare/kumo/components/select";
import { useTranslation } from "react-i18next";
import { DialogBody, DialogFooter, DialogHeader, dialogPanelClass } from "./DialogLayout";
import { InlineMessage } from "./InlineMessage";
import { apiGet, apiPut } from "../lib/api";

type Role = "member" | "audit_reader" | "device_group_operator";
type Assignment = { role: "member" | "audit_reader" } | { role: "device_group_operator"; group_ids: number[] };

export function ConsoleRoleAction({ id, username }: { id: number; username: string }) {
  const { t } = useTranslation();
  const [open, setOpen] = useState(false);
  return (
    <>
      <Button size="sm" variant="secondary" onClick={() => setOpen(true)}>{t("consoleRole")}</Button>
      <Dialog.Root open={open} onOpenChange={setOpen}>
        <Dialog size="base" className={dialogPanelClass}>
          <DialogHeader title={t("consoleRoleFor", { username })} />
          {open && <RoleForm id={id} close={() => setOpen(false)} />}
        </Dialog>
      </Dialog.Root>
    </>
  );
}

function RoleForm({ id, close }: { id: number; close: () => void }) {
  const { t } = useTranslation();
  const client = useQueryClient();
  const path = `/api/admin/console-role/${id}`;
  const current = useQuery({ queryKey: [path], queryFn: () => apiGet<Assignment>(path) });
  const [draft, setDraft] = useState<Assignment>();
  const [page, setPage] = useState(1);
  const assignment = draft ?? current.data;
  const groupIds = assignment?.role === "device_group_operator" ? assignment.group_ids : [];
  const groups = useQuery({
    queryKey: ["console-role-device-groups", page],
    queryFn: () => apiGet<{ list: { id: number; name: string }[]; total: number }>("/api/admin/device_group/list", { page, page_size: 10 }),
    enabled: assignment?.role === "device_group_operator",
  });
  const save = useMutation({
    mutationFn: (value: Assignment) => apiPut(path, value),
    onSuccess: (_, value) => {
      client.setQueryData([path], value);
      close();
    },
  });
  const labels: Record<Role, string> = {
    member: t("consoleRoleMember"),
    audit_reader: t("consoleRoleAuditReader"),
    device_group_operator: t("consoleRoleOperator"),
  };

  return (
    <>
      <DialogBody>
        <div className="space-y-4">
          {current.isPending && <p role="status">{t("loading")}</p>}
          {(current.isError || groups.isError || save.isError) && <InlineMessage tone="error">{t("operationFailed")}</InlineMessage>}
          {assignment && (
            <>
              <Select<Role>
                label={t("consoleRole")}
                value={assignment.role}
                items={(Object.keys(labels) as Role[]).map((role) => ({ value: role, label: labels[role] }))}
                onValueChange={(role) => {
                  if (role) setDraft(role === "device_group_operator" ? { role, group_ids: groupIds } : { role });
                }}
              >
                {(Object.keys(labels) as Role[]).map((role) => (
                  <Select.Option key={role} value={role}>{labels[role]}</Select.Option>
                ))}
              </Select>
              {assignment.role === "device_group_operator" && (
                <fieldset className="space-y-3">
                  <legend className="text-sm font-medium">{t("consoleAssignedGroups")}</legend>
                  <p className="text-sm text-kumo-subtle">{t("consoleGroupCount", { count: groupIds.length })}</p>
                  {groups.isPending && <p role="status">{t("loading")}</p>}
                  {groups.data?.list.map((group) => (
                    <Checkbox
                      key={group.id}
                      label={group.name}
                      checked={groupIds.includes(group.id)}
                      disabled={groupIds.length >= 50 && !groupIds.includes(group.id)}
                      onCheckedChange={(checked) => setDraft({
                        role: "device_group_operator",
                        group_ids: checked ? [...groupIds, group.id] : groupIds.filter((id) => id !== group.id),
                      })}
                    />
                  ))}
                  {groups.data && groups.data.total > 10 && (
                    <div className="flex gap-2">
                      <Button variant="secondary" disabled={page <= 1} onClick={() => setPage(page - 1)}>{t("consolePreviousPage")}</Button>
                      <Button variant="secondary" disabled={page * 10 >= groups.data.total} onClick={() => setPage(page + 1)}>{t("consoleNextPage")}</Button>
                    </div>
                  )}
                </fieldset>
              )}
            </>
          )}
        </div>
      </DialogBody>
      <DialogFooter>
        <Button variant="secondary" disabled={save.isPending} onClick={close}>{t("cancel")}</Button>
        <Button variant="primary" loading={save.isPending} disabled={!draft || (draft.role === "device_group_operator" && groupIds.length === 0)} onClick={() => draft && save.mutate(draft)}>
          {t("save")}
        </Button>
      </DialogFooter>
    </>
  );
}
