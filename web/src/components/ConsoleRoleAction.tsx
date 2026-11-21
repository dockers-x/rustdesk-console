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

type Assignment =
  | { role: "member" | "audit_reader" }
  | { role: "device_group_operator"; group_ids: number[] }
  | { role: "scoped_administrator"; group_ids: number[]; collection_ids: number[] };
type Role = Assignment["role"];

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
  const assignment = draft ?? current.data;
  const groupIds = assignment && "group_ids" in assignment ? assignment.group_ids : [];
  const collectionIds = assignment?.role === "scoped_administrator" ? assignment.collection_ids : [];
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
    scoped_administrator: t("consoleRoleScopedAdministrator"),
  };

  return (
    <>
      <DialogBody>
        <div className="space-y-4">
          {current.isPending && <p role="status">{t("loading")}</p>}
          {(current.isError || save.isError) && <InlineMessage tone="error">{t("operationFailed")}</InlineMessage>}
          {assignment && (
            <>
              <Select<Role>
                label={t("consoleRole")}
                value={assignment.role}
                items={(Object.keys(labels) as Role[]).map((role) => ({ value: role, label: labels[role] }))}
                onValueChange={(role) => {
                  if (role === "device_group_operator") setDraft({ role, group_ids: [] });
                  else if (role === "scoped_administrator") setDraft({ role, group_ids: [], collection_ids: [] });
                  else if (role) setDraft({ role });
                }}
              >
                {(Object.keys(labels) as Role[]).map((role) => (
                  <Select.Option key={role} value={role}>{labels[role]}</Select.Option>
                ))}
              </Select>
              {assignment.role === "device_group_operator" && (
                <ScopePicker
                  key="device-groups"
                  path="/api/admin/device_group/list"
                  label={t("consoleAssignedGroups")}
                  ids={groupIds}
                  onChange={(group_ids) => setDraft({ ...assignment, group_ids })}
                />
              )}
              {assignment.role === "scoped_administrator" && (
                <>
                  <p className="text-sm text-kumo-subtle">{t("delegationAssignmentHint")}</p>
                  <ScopePicker
                    key="user-groups"
                    path="/api/admin/group/list"
                    label={t("delegationUserGroups")}
                    ids={groupIds}
                    onChange={(group_ids) => setDraft({ ...assignment, group_ids })}
                  />
                  <ScopePicker
                    path="/api/admin/delegation-collections"
                    label={t("delegationSharedCollections")}
                    ids={collectionIds}
                    onChange={(collection_ids) => setDraft({ ...assignment, collection_ids })}
                  />
                </>
              )}
            </>
          )}
        </div>
      </DialogBody>
      <DialogFooter>
        <Button variant="secondary" disabled={save.isPending} onClick={close}>{t("cancel")}</Button>
        <Button variant="primary" loading={save.isPending} disabled={!draft
          || (draft.role === "device_group_operator" && groupIds.length === 0)
          || (draft.role === "scoped_administrator" && groupIds.length + collectionIds.length === 0)} onClick={() => draft && save.mutate(draft)}>
          {t("save")}
        </Button>
      </DialogFooter>
    </>
  );
}


function ScopePicker({ path, label, ids, onChange }: {
  path: string;
  label: string;
  ids: number[];
  onChange: (ids: number[]) => void;
}) {
  const { t } = useTranslation();
  const [page, setPage] = useState(1);
  const options = useQuery({
    queryKey: [path, "role-options", page],
    queryFn: () => apiGet<{ list: { id: number; name: string }[]; total: number }>(path, { page, page_size: 10 }),
  });

  return (
    <fieldset className="space-y-2 rounded-lg border border-kumo-line px-3 pb-3">
      <legend className="px-1 text-sm font-medium">{label}</legend>
      <p className="text-sm text-kumo-subtle">{t("consoleGroupCount", { count: ids.length })}</p>
      {options.isPending && <p role="status">{t("loading")}</p>}
      {options.isError && <InlineMessage tone="error">{t("operationFailed")}</InlineMessage>}
      {options.data?.total === 0 && <p className="text-sm text-kumo-subtle">{t("noData")}</p>}
      <div className="grid gap-2">
        {options.data?.list.map((option) => (
          <Checkbox
            key={option.id}
            label={`${option.name} (#${option.id})`}
            checked={ids.includes(option.id)}
            disabled={ids.length >= 50 && !ids.includes(option.id)}
            onCheckedChange={(checked) => onChange(checked ? [...ids, option.id] : ids.filter((id) => id !== option.id))}
          />
        ))}
      </div>
      {(page > 1 || (options.data?.total ?? 0) > 10) && (
        <div className="flex flex-wrap gap-2">
          <Button size="sm" variant="secondary" disabled={page <= 1} onClick={() => setPage(page - 1)}>{t("consolePreviousPage")}</Button>
          <Button size="sm" variant="secondary" disabled={page * 10 >= (options.data?.total ?? 0)} onClick={() => setPage(page + 1)}>{t("consoleNextPage")}</Button>
        </div>
      )}
    </fieldset>
  );
}
