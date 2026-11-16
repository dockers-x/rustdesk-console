/**
 * Copies text to the clipboard.
 *
 * The async Clipboard API is only available in secure contexts (HTTPS or
 * localhost). The admin panel is commonly served over plain HTTP on a LAN
 * address, where `navigator.clipboard` is undefined. Fall back to the legacy
 * `execCommand("copy")` path so the copy buttons keep working there.
 */
export async function copyToClipboard(value: string): Promise<boolean> {
  if (!value) return false;
  if (window.isSecureContext && navigator.clipboard?.writeText) {
    try {
      await navigator.clipboard.writeText(value);
      return true;
    } catch {
      // Permission denied or document not focused: try the legacy path below.
    }
  }
  return copyWithExecCommand(value);
}

function copyWithExecCommand(value: string): boolean {
  const area = document.createElement("textarea");
  area.value = value;
  area.setAttribute("readonly", "");
  area.style.position = "fixed";
  area.style.top = "0";
  area.style.left = "0";
  area.style.width = "1px";
  area.style.height = "1px";
  area.style.padding = "0";
  area.style.border = "none";
  area.style.outline = "none";
  area.style.boxShadow = "none";
  area.style.background = "transparent";
  area.style.opacity = "0";

  const selection = document.getSelection();
  const previousRange =
    selection && selection.rangeCount > 0 ? selection.getRangeAt(0) : null;

  document.body.appendChild(area);
  let ok = false;
  try {
    area.focus();
    area.select();
    area.setSelectionRange(0, value.length);
    ok = document.execCommand("copy");
  } catch {
    ok = false;
  } finally {
    document.body.removeChild(area);
    if (previousRange && selection) {
      selection.removeAllRanges();
      selection.addRange(previousRange);
    }
  }
  return ok;
}
