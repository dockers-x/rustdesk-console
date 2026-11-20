import {
  useId,
  useLayoutEffect,
  useRef,
  useState,
  type ChangeEvent,
  type RefObject,
  type SyntheticEvent,
} from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Badge } from "@cloudflare/kumo/components/badge";
import { Button } from "@cloudflare/kumo/components/button";
import { Dialog } from "@cloudflare/kumo/components/dialog";
import { Input, Textarea } from "@cloudflare/kumo/components/input";
import { Select } from "@cloudflare/kumo/components/select";
import { Table } from "@cloudflare/kumo/components/table";
import { XIcon } from "@phosphor-icons/react";
import { useTranslation } from "react-i18next";
import { ConfirmDialog } from "../components/ConfirmDialog";
import {
  DialogBody,
  DialogFooter,
  DialogHeader,
  dialogPanelClass,
} from "../components/DialogLayout";
import { InlineMessage } from "../components/InlineMessage";
import { TableState } from "../components/TableState";
import { apiGet, apiPost } from "../lib/api";

const VARIABLE_LABELS = {
  device_id: "deviceId",
  device_name: "notificationVariableDeviceName",
  hostname: "hostname",
  platform: "platform",
  username: "username",
  device_group: "notificationVariableDeviceGroup",
  event_type: "event",
  state: "notificationVariableState",
  previous_state: "notificationVariablePreviousState",
  occurred_at: "notificationVariableOccurredAt",
  last_seen_at: "notificationVariableLastSeenAt",
} as const;

type TemplateVariable = keyof typeof VARIABLE_LABELS;
type Segment =
  | { type: "text"; text: string }
  | { type: "variable"; name: TemplateVariable };

interface NotificationTemplate {
  id: number;
  name: string;
  title: Segment[];
  body: Segment[];
}

interface TemplateList {
  list: NotificationTemplate[];
  total: number;
  page_size: number;
  variables: TemplateVariable[];
}

const TEMPLATES_QUERY = ["notification-templates"];
const TEMPLATES_PATH = "/api/admin/notification/templates";
const EMPTY_TEMPLATE = { id: 0, name: "", title: [], body: [] };
const TITLE_LIMIT = 256;
const BODY_LIMIT = 4000;
const SEGMENT_LIMIT = 128;
const DEFAULT_VARIABLES = Object.keys(VARIABLE_LABELS) as TemplateVariable[];

interface TextPart {
  id: number;
  text: string;
  variable?: TemplateVariable;
}

interface TemplateDraft {
  id: number;
  name: string;
  title: TextPart[];
  body: TextPart[];
}

function editableParts(segments: Segment[]): TextPart[] {
  const parts: TextPart[] = [];
  let text = "";
  for (const segment of segments) {
    if (segment.type === "text") {
      text += segment.text;
    } else {
      parts.push({ id: parts.length, text, variable: segment.name });
      text = "";
    }
  }
  return [...parts, { id: parts.length, text }];
}

function toSegments(parts: TextPart[]): Segment[] {
  const segments: Segment[] = [];
  for (const part of parts) {
    if (part.text) segments.push({ type: "text", text: part.text });
    if (part.variable) segments.push({ type: "variable", name: part.variable });
  }
  return segments;
}

function textLength(parts: TextPart[]) {
  return parts.reduce((sum, part) => sum + Array.from(part.text).length, 0);
}

function containsMessage(parts: TextPart[]) {
  return parts.some((part) => part.variable || part.text.trim());
}

type TextControl = HTMLInputElement | HTMLTextAreaElement;
interface TextPosition {
  id: number;
  offset: number;
}

function TemplateFieldEditor({
  label,
  parts,
  variables,
  pickerPortal,
  limit,
  multiline = false,
  onChange,
}: {
  label: string;
  parts: TextPart[];
  variables: TemplateVariable[];
  pickerPortal: RefObject<HTMLDivElement | null>;
  limit: number;
  multiline?: boolean;
  onChange: (parts: TextPart[]) => void;
}) {
  const { t } = useTranslation();
  const countId = useId();
  const controls = useRef(new Map<number, TextControl>());
  const cursor = useRef<TextPosition | null>(null);
  const pendingFocus = useRef<TextPosition | null>(null);
  const waitForPicker = useRef(false);
  const used = textLength(parts);
  const segmentCount = toSegments(parts).length;

  const restoreFocus = () => {
    if (!pendingFocus.current || waitForPicker.current) return;
    const { id, offset } = pendingFocus.current;
    const control = controls.current.get(id);
    if (!control) return;
    pendingFocus.current = null;
    control.focus();
    control.setSelectionRange(offset, offset);
    cursor.current = { id, offset };
  };

  useLayoutEffect(restoreFocus, [parts]);

  const insertVariable = (variable: TemplateVariable) => {
    const index = parts.findIndex((part) => part.id === cursor.current?.id);
    const position = index < 0 ? parts.length - 1 : index;
    const part = parts[position];
    const offset = index < 0 ? part.text.length : cursor.current?.offset ?? 0;
    const nextId = Math.max(...parts.map((item) => item.id)) + 1;
    waitForPicker.current = true;
    pendingFocus.current = { id: nextId, offset: 0 };
    onChange([
      ...parts.slice(0, position),
      { ...part, text: part.text.slice(0, offset), variable },
      { ...part, id: nextId, text: part.text.slice(offset) },
      ...parts.slice(position + 1),
    ]);
  };

  const removeVariable = (index: number) => {
    const before = parts[index];
    const after = parts[index + 1];
    pendingFocus.current = { id: before.id, offset: before.text.length };
    onChange([
      ...parts.slice(0, index),
      { ...before, text: before.text + after.text, variable: after.variable },
      ...parts.slice(index + 2),
    ]);
  };

  return (
    <fieldset className="min-w-0">
      <legend className="mb-1 text-sm">{label}</legend>
      <div className="grid gap-2">
        {parts.map((part, index) => {
          const rememberCaret = (event: SyntheticEvent<TextControl>) => {
            const control = event.currentTarget;
            cursor.current = { id: part.id, offset: control.selectionEnd ?? control.value.length };
          };
          const controlProps = {
            ref: (control: TextControl | null) => {
              if (control) controls.current.set(part.id, control);
              else controls.current.delete(part.id);
            },
            "aria-label": parts.length === 1
              ? label
              : t("notificationTemplateTextPart", { field: label, position: index + 1 }),
            "aria-describedby": countId,
            "aria-invalid": used > limit || segmentCount > SEGMENT_LIMIT,
            value: part.text,
            onChange: (event: ChangeEvent<TextControl>) => {
              rememberCaret(event);
              onChange(parts.map((item) =>
                item.id === part.id ? { ...item, text: event.target.value } : item,
              ));
            },
            onFocus: rememberCaret,
            onSelect: rememberCaret,
            onBlur: rememberCaret,
          };
          return (
            <div key={part.id} className="grid gap-2">
              {multiline ? (
                <Textarea
                  {...controlProps}
                  rows={parts.length === 1 ? 3 : 1}
                  className={`w-full ${parts.length === 1 ? "min-h-24" : "min-h-9"} resize-y [field-sizing:content]`}
                />
              ) : (
                <Input {...controlProps} className="w-full" />
              )}
              {part.variable && (
                <Button
                  type="button"
                  size="sm"
                  variant="secondary"
                  className="min-h-9 justify-self-start"
                  aria-label={t("notificationTemplateRemoveVariable", {
                    variable: t(VARIABLE_LABELS[part.variable]),
                  })}
                  onClick={() => removeVariable(index)}
                >
                  <Badge variant="secondary">{t(VARIABLE_LABELS[part.variable])}</Badge>
                  <XIcon aria-hidden="true" size={14} />
                </Button>
              )}
            </div>
          );
        })}
      </div>
      <div className="mt-1.5 flex flex-wrap items-center justify-between gap-2">
        <Select<TemplateVariable>
          className="max-w-full [&_[data-placeholder]]:text-kumo-default!"
          aria-label={t("notificationTemplateInsertVariableIn", { field: label })}
          container={pickerPortal}
          disabled={segmentCount >= SEGMENT_LIMIT}
          placeholder={t("notificationTemplateInsertVariable")}
          value={null}
          items={variables.map((variable) => ({
            value: variable,
            label: t(VARIABLE_LABELS[variable]),
          }))}
          onValueChange={(variable) => {
            if (variable) insertVariable(variable);
          }}
          onOpenChangeComplete={(open) => {
            if (!open) {
              waitForPicker.current = false;
              restoreFocus();
            }
          }}
        >
          {variables.map((variable) => (
            <Select.Option key={variable} value={variable}>
              <Badge variant="secondary">{t(VARIABLE_LABELS[variable])}</Badge>
            </Select.Option>
          ))}
        </Select>
        <p
          id={countId}
          className={`text-xs ${used > limit || segmentCount > SEGMENT_LIMIT ? "text-kumo-danger" : "text-kumo-default/80"}`}
        >
          {segmentCount > SEGMENT_LIMIT
            ? t("notificationTemplateSegmentLimit", { limit: SEGMENT_LIMIT })
            : t("notificationTemplateCharacterCount", { used, limit })}
        </p>
      </div>
    </fieldset>
  );
}

function TemplateMessage({ segments }: { segments: Segment[] }) {
  const { t } = useTranslation();
  return (
    <span className="whitespace-pre-wrap [overflow-wrap:anywhere]">
      {segments.map((segment, index) =>
        segment.type === "text" ? segment.text : (
          <Badge key={index} variant="secondary" className="align-baseline">
            {t(VARIABLE_LABELS[segment.name])}
          </Badge>
        ),
      )}
    </span>
  );
}

export function useNotificationTemplates() {
  return useQuery({
    queryKey: TEMPLATES_QUERY,
    queryFn: async () => {
      const loadPage = (page: number) =>
        apiGet<TemplateList>(TEMPLATES_PATH, { page, page_size: 100 });
      const first = await loadPage(1);
      const additionalPages = Math.ceil(first.total / first.page_size) - 1;
      const rest = await Promise.all(
        Array.from({ length: additionalPages }, (_, index) => loadPage(index + 2)),
      );
      return {
        list: [...first.list, ...rest.flatMap((page) => page.list)],
        variables: first.variables,
      };
    },
  });
}

export function NotificationTemplatesPanel() {
  const { t } = useTranslation();
  const qc = useQueryClient();
  const templates = useNotificationTemplates();
  const pickerPortal = useRef<HTMLDivElement>(null);
  const [form, setForm] = useState<TemplateDraft | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<NotificationTemplate | null>(null);
  const [message, setMessage] = useState("");

  const save = useMutation({
    mutationFn: (template: NotificationTemplate) =>
      apiPost<NotificationTemplate>(TEMPLATES_PATH, template),
    onSuccess: () => {
      setForm(null);
      setMessage(t("notificationTemplateSaved"));
      void qc.invalidateQueries({ queryKey: TEMPLATES_QUERY });
    },
  });
  const remove = useMutation({
    mutationFn: (id: number) =>
      apiPost("/api/admin/notification/template/delete", { id }),
    onSuccess: () => {
      setDeleteTarget(null);
      setMessage(t("notificationTemplateDeleted"));
      void qc.invalidateQueries({ queryKey: TEMPLATES_QUERY });
    },
  });

  const openEditor = (template: NotificationTemplate) => {
    save.reset();
    setMessage("");
    setForm({
      id: template.id,
      name: template.name,
      title: editableParts(template.title),
      body: editableParts(template.body),
    });
  };
  const title = form ? toSegments(form.title) : [];
  const body = form ? toSegments(form.body) : [];
  const canSave =
    form?.name.trim() &&
    containsMessage(form.title) &&
    containsMessage(form.body) &&
    textLength(form.title) <= TITLE_LIMIT &&
    textLength(form.body) <= BODY_LIMIT &&
    title.length <= SEGMENT_LIMIT &&
    body.length <= SEGMENT_LIMIT;
  const variables = templates.data?.variables ?? DEFAULT_VARIABLES;

  return (
    <section className="mt-5 rounded-lg border border-kumo-line bg-kumo-elevated">
      <div className="flex flex-wrap items-start justify-between gap-3 border-b border-kumo-line px-4 py-3">
        <div>
          <h2 className="text-base font-semibold">{t("notificationTemplates")}</h2>
          <p className="mt-1 text-sm text-kumo-subtle">
            {t("notificationTemplatesHint")}
          </p>
        </div>
        <Button size="sm" onClick={() => openEditor(EMPTY_TEMPLATE)}>
          {t("notificationTemplateCreate")}
        </Button>
      </div>
      {message && (
        <InlineMessage tone="success" className="mx-4 mt-3">
          {message}
        </InlineMessage>
      )}
      <div className="overflow-x-auto">
        <Table>
          <Table.Header>
            <Table.Row>
              <Table.Head>{t("name")}</Table.Head>
              <Table.Head>{t("notificationTemplateTitle")}</Table.Head>
              <Table.Head>{t("actions")}</Table.Head>
            </Table.Row>
          </Table.Header>
          <Table.Body>
            {templates.data?.list.map((template) => (
              <Table.Row key={template.id}>
                <Table.Cell className="font-medium">{template.name}</Table.Cell>
                <Table.Cell className="max-w-md">
                  <TemplateMessage segments={template.title} />
                </Table.Cell>
                <Table.Cell>
                  <div className="flex flex-wrap gap-1">
                    <Button size="sm" variant="ghost" onClick={() => openEditor(template)}>
                      {t("edit")}
                    </Button>
                    <Button
                      size="sm"
                      variant="secondary-destructive"
                      onClick={() => {
                        remove.reset();
                        setDeleteTarget(template);
                      }}
                    >
                      {t("delete")}
                    </Button>
                  </div>
                </Table.Cell>
              </Table.Row>
            ))}
          </Table.Body>
        </Table>
        {templates.isLoading && (
          <TableState tone="loading">{t("loading")}</TableState>
        )}
        {templates.error && (
          <TableState tone="error">
            {templates.error.message || t("operationFailed")}
          </TableState>
        )}
        {templates.data?.list.length === 0 && (
          <TableState tone="empty">{t("notificationTemplatesEmpty")}</TableState>
        )}
      </div>

      <Dialog.Root
        open={form !== null}
        onOpenChange={(open) => {
          if (!open && !save.isPending) setForm(null);
        }}
      >
        {form && (
          <Dialog size="lg" className={dialogPanelClass} style={{ overflow: "visible" }}>
            <DialogHeader
              title={t(form.id ? "notificationTemplateEdit" : "notificationTemplateCreate")}
            />
            <DialogBody>
              <form
                id="notification-template-form"
                className="grid gap-3"
                onSubmit={(event) => {
                  event.preventDefault();
                  if (canSave && !save.isPending) {
                    save.mutate({ id: form.id, name: form.name, title, body });
                  }
                }}
              >
                <label className="block">
                  <span className="mb-1 block text-sm">{t("name")}</span>
                  <Input
                    required
                    className="w-full"
                    value={form.name}
                    onChange={(event) => setForm({ ...form, name: event.target.value })}
                  />
                </label>
                <p className="text-sm text-kumo-default/80">
                  {t("notificationTemplateVariablesHint")}
                </p>
                <TemplateFieldEditor
                  label={t("notificationTemplateTitle")}
                  parts={form.title}
                  variables={variables}
                  pickerPortal={pickerPortal}
                  limit={TITLE_LIMIT}
                  onChange={(parts) => setForm({ ...form, title: parts })}
                />
                <TemplateFieldEditor
                  label={t("notificationTemplateBody")}
                  parts={form.body}
                  variables={variables}
                  pickerPortal={pickerPortal}
                  limit={BODY_LIMIT}
                  multiline
                  onChange={(parts) => setForm({ ...form, body: parts })}
                />
                {(title.length > 0 || body.length > 0) && (
                  <details className="rounded-lg border border-kumo-line bg-kumo-recessed p-3 text-sm">
                    <summary className="cursor-pointer rounded-sm font-medium focus-visible:outline-2">
                      {t("notificationTemplatePreview")}
                    </summary>
                    <div className="mt-2 grid gap-1 border-t border-kumo-line pt-2">
                      <div className="font-semibold"><TemplateMessage segments={title} /></div>
                      <div><TemplateMessage segments={body} /></div>
                    </div>
                  </details>
                )}
              </form>
            </DialogBody>
            <div className="shrink-0 overflow-hidden rounded-b-xl">
              <DialogFooter error={save.error && (save.error.message || t("operationFailed"))}>
                <Button
                  variant="secondary"
                  disabled={save.isPending}
                  onClick={() => setForm(null)}
                >
                  {t("cancel")}
                </Button>
                <Button
                  type="submit"
                  form="notification-template-form"
                  loading={save.isPending}
                  disabled={!canSave || save.isPending}
                >
                  {t("save")}
                </Button>
              </DialogFooter>
            </div>
            <div ref={pickerPortal} className="contents" />
          </Dialog>
        )}
      </Dialog.Root>

      <ConfirmDialog
        open={deleteTarget !== null}
        title={t("notificationTemplateDelete")}
        description={t("notificationTemplateDeleteHint", { name: deleteTarget?.name })}
        confirmLabel={t("delete")}
        cancelLabel={t("cancel")}
        loading={remove.isPending}
        error={remove.error?.message}
        onOpenChange={(open) => {
          if (!open && !remove.isPending) setDeleteTarget(null);
        }}
        onConfirm={() => {
          if (deleteTarget) remove.mutate(deleteTarget.id);
        }}
      />
    </section>
  );
}
