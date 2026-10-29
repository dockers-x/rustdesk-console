import { Fragment, useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";
import chinese from "../../../../docs/self-signed-client.zh-CN.md?raw";
import english from "../../../../docs/self-signed-client.en.md?raw";

function inline(text: string): ReactNode {
  return text.split(/(`[^`]+`|\*\*[^*]+\*\*)/g).map((part, index) => part.startsWith("`") && part.endsWith("`") ? (
    <code key={index} className="break-words rounded bg-kumo-tint/60 px-1 py-0.5 font-mono text-[0.9em]">{part.slice(1, -1)}</code>
  ) : part.startsWith("**") && part.endsWith("**") ? <strong key={index}>{part.slice(2, -2)}</strong> : part);
}

function Markdown({ text }: { text: string }) {
  const lines = text.split("\n");
  const blocks: ReactNode[] = [];
  let position = 0;
  while (position < lines.length) {
    const line = lines[position];
    const key = position;
    if (!line.trim()) { position += 1; continue; }
    if (line.startsWith("```")) {
      const code: string[] = [];
      position += 1;
      while (position < lines.length && !lines[position].startsWith("```")) code.push(lines[position++]);
      position += 1;
      blocks.push(<pre key={key} className="max-w-full overflow-x-auto rounded-lg border border-kumo-line bg-kumo-tint/60 p-4 text-xs leading-6"><code>{code.join("\n")}</code></pre>);
      continue;
    }
    const heading = /^(#{1,6})\s+(.+)$/.exec(line);
    if (heading) {
      const level = heading[1].length;
      blocks.push(level === 1 ? <h2 key={key} className="text-xl font-semibold tracking-tight">{inline(heading[2])}</h2> : <h3 key={key} className="pt-3 text-base font-semibold">{inline(heading[2])}</h3>);
      position += 1;
      continue;
    }
    if (line.startsWith("|") && /^\|[\s:|-]+\|\s*$/.test(lines[position + 1] ?? "")) {
      const cells = (row: string) => row.trim().replace(/^\||\|$/g, "").split("|").map((cell) => cell.trim());
      const headings = cells(line);
      const rows: string[][] = [];
      position += 2;
      while (position < lines.length && lines[position].startsWith("|")) rows.push(cells(lines[position++]));
      blocks.push(<div key={key} className="max-w-full overflow-x-auto rounded-lg border border-kumo-line"><table className="w-full text-left text-sm"><thead className="bg-kumo-tint/60"><tr>{headings.map((cell, index) => <th key={index} className="px-3 py-2 font-medium">{inline(cell)}</th>)}</tr></thead><tbody>{rows.map((row, index) => <tr key={index} className="border-t border-kumo-line">{row.map((cell, cellIndex) => <td key={cellIndex} className="min-w-32 px-3 py-2 align-top">{inline(cell)}</td>)}</tr>)}</tbody></table></div>);
      continue;
    }
    const list = /^(?:[-*]|\d+\.)\s+/.exec(line);
    if (list) {
      const ordered = /^\d/.test(line);
      const items: ReactNode[] = [];
      const pattern = ordered ? /^\d+\.\s+/ : /^[-*]\s+/;
      while (position < lines.length && pattern.test(lines[position])) {
        items.push(<li key={position}>{inline(lines[position++].replace(pattern, ""))}</li>);
      }
      const className = "space-y-2 pl-6 text-sm leading-7 " + (ordered ? "list-decimal" : "list-disc");
      blocks.push(ordered ? <ol key={key} className={className}>{items}</ol> : <ul key={key} className={className}>{items}</ul>);
      continue;
    }
    blocks.push(<p key={key} className="text-sm leading-7">{inline(line)}</p>);
    position += 1;
  }
  return <>{blocks.map((block, index) => <Fragment key={index}>{block}</Fragment>)}</>;
}

export function Documentation() {
  const { t, i18n } = useTranslation();
  const [chosenLanguage, setChosenLanguage] = useState<string | null>(null);
  const language = chosenLanguage ?? ((i18n.resolvedLanguage ?? i18n.language).startsWith("zh") ? "zh-CN" : "en");
  const text = language === "zh-CN" ? chinese : english;
  return (
    <section className="min-w-0 space-y-5">
      <div className="flex flex-wrap items-end justify-between gap-3 border-b border-kumo-line pb-4">
        <label className="flex flex-col gap-1.5 text-sm font-medium">
          {t("selfSignedDocsLanguage")}
          <select value={language} onChange={(event) => setChosenLanguage(event.target.value)} className="min-h-11 rounded-md border border-kumo-line bg-kumo-base px-3 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand">
            <option value="zh-CN">{t("selfSignedDocsChinese")}</option>
            <option value="en">{t("selfSignedDocsEnglish")}</option>
          </select>
        </label>
        <a href={`data:text/markdown;charset=utf-8,${encodeURIComponent(text)}`} download={`self-signed-client.${language}.md`} className="inline-flex min-h-11 items-center rounded-md border border-kumo-line px-3 text-sm font-medium hover:bg-kumo-tint/60 focus-visible:outline-none focus-visible:ring-2 focus-visible:ring-kumo-brand">{t("selfSignedDocsDownload")}</a>
      </div>
      <article className="min-w-0 max-w-4xl space-y-4 break-words"><Markdown text={text} /></article>
    </section>
  );
}
