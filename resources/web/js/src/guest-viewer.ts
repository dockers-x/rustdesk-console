import Connection from "./connection";
import { guestLaunch, guestMode } from "./guest";
import type { DisplayInfo } from "./message";

const en = {
  title: "Guest remote desktop",
  disconnect: "Disconnect",
  fullscreen: "Full screen",
  display: "Display",
  password: "Device password",
  submit: "Connect",
  canvas: "Remote screen",
  inputText: "Send text to device",
  connecting: "Connecting to the device…",
  approval: "Enter the device password or wait for approval.",
  wrongPassword: "That password did not work. Try again.",
  active: "Connected",
  closed: "Session ended",
  unavailable: "The guest session is unavailable. Reopen the link or ask for a new one.",
};
const zh = {
  title: "访客远程桌面",
  disconnect: "断开连接",
  fullscreen: "全屏",
  display: "显示器",
  password: "设备密码",
  submit: "连接",
  canvas: "远程画面",
  inputText: "向设备输入文字",
  connecting: "正在连接设备…",
  approval: "请输入设备密码，或等待设备上的人员批准。",
  wrongPassword: "密码不正确，请重试。",
  active: "已连接",
  closed: "会话已结束",
  unavailable: "访客会话不可用。请重新打开访客链接，或联系管理员获取新链接。",
};
const ru = {
  title: "Гостевой удалённый рабочий стол",
  disconnect: "Отключиться",
  fullscreen: "На весь экран",
  display: "Экран",
  password: "Пароль устройства",
  submit: "Подключиться",
  canvas: "Удалённый экран",
  inputText: "Отправить текст на устройство",
  connecting: "Подключение к устройству…",
  approval: "Введите пароль устройства или дождитесь подтверждения.",
  wrongPassword: "Пароль не подошёл. Попробуйте снова.",
  active: "Подключено",
  closed: "Сеанс завершён",
  unavailable: "Гостевой сеанс недоступен. Откройте ссылку снова или запросите новую.",
};

type CopyKey = keyof typeof en;
type GuestWindow = Window & {
  onGlobalEvent?: (event: string) => void;
  init?: () => Promise<void>;
  YUVCanvas: {
    attach(canvas: HTMLCanvasElement): { drawFrame(frame: unknown): void };
  };
};

const controlKeys: Record<string, string> = {
  ArrowLeft: "LeftArrow",
  ArrowRight: "RightArrow",
  ArrowUp: "UpArrow",
  ArrowDown: "DownArrow",
  Enter: "Return",
  Escape: "Escape",
  Backspace: "Backspace",
  Delete: "Delete",
  Home: "Home",
  End: "End",
  PageUp: "PageUp",
  PageDown: "PageDown",
  Insert: "Insert",
  Alt: "Alt",
  Control: "Control",
  Shift: "Shift",
  Meta: "Meta",
};

function copy() {
  const lang = (navigator.language || "").toLowerCase();
  if (lang.startsWith("zh")) return zh;
  return lang.startsWith("ru") ? ru : en;
}

function startViewer(root: HTMLElement) {
  const text = copy();
  const browser = window as unknown as GuestWindow;
  const peer = root.querySelector<HTMLElement>("#guest-peer")!;
  const status = root.querySelector<HTMLElement>("#guest-status")!;
  const disconnect = root.querySelector<HTMLButtonElement>("#guest-disconnect")!;
  const fullscreen = root.querySelector<HTMLButtonElement>("#guest-fullscreen")!;
  const display = root.querySelector<HTMLSelectElement>("#guest-display")!;
  const displayField = root.querySelector<HTMLElement>("#guest-display-field")!;
  const passwordForm = root.querySelector<HTMLFormElement>("#guest-password-form")!;
  const password = root.querySelector<HTMLInputElement>("#guest-password")!;
  const canvas = root.querySelector<HTMLCanvasElement>("#guest-canvas")!;
  const textInput = root.querySelector<HTMLTextAreaElement>("#guest-text-input");

  document.title = text.title;
  document.documentElement.lang = text === zh ? "zh-CN" : text === ru ? "ru" : "en";
  root.querySelectorAll<HTMLElement>("[data-guest-label]").forEach((element) => {
    const key = element.dataset.guestLabel as CopyKey;
    if (element instanceof HTMLCanvasElement) {
      element.setAttribute("aria-label", text[key]);
    } else {
      element.textContent = text[key];
    }
  });
  const show = (key: CopyKey, tone = "neutral") => {
    status.textContent = text[key];
    status.dataset.tone = tone;
  };
  if (!guestLaunch) {
    disconnect.disabled = true;
    fullscreen.hidden = true;
    canvas.hidden = true;
    show("unavailable", "error");
    return;
  }

  const connection = new Connection();
  const video = browser.YUVCanvas.attach(canvas);
  const pressed = new Set<string>();
  let displays: DisplayInfo[] = [];
  let screen = 0;
  let active = false;
  let ended = false;

  peer.textContent = guestLaunch.peer_id;
  fullscreen.hidden = !document.fullscreenEnabled;
  disconnect.disabled = false;
  show("connecting");

  function end(key: "closed" | "unavailable") {
    if (ended) return;
    ended = true;
    active = false;
    pressed.clear();
    connection.close();
    password.value = "";
    passwordForm.hidden = true;
    canvas.hidden = true;
    displayField.hidden = true;
    if (textInput) textInput.hidden = true;
    disconnect.disabled = true;
    show(key, key === "unavailable" ? "error" : "neutral");
  }

  browser.onGlobalEvent = (raw) => {
    const event = JSON.parse(raw) as Record<string, string>;
    if (event.name === "peer_info") {
      displays = JSON.parse(event.displays || "[]") as DisplayInfo[];
      screen = Number(event.current_display) || 0;
      display.replaceChildren(...displays.map((item, index) =>
        new Option(item.name ? `${index + 1} · ${item.name}` : String(index + 1), String(index))));
      display.value = String(screen);
      displayField.hidden = displays.length < 2;
      active = true;
      passwordForm.hidden = true;
      if (textInput && matchMedia("(pointer: coarse)").matches) textInput.hidden = false;
      canvas.focus({ preventScroll: true });
      show("active", "success");
    } else if (event.name === "connection_ready") {
      show("approval");
    }
  };

  connection.setMsgbox((kind) => {
    if (ended) return;
    if (kind === "input-password" || kind === "re-input-password") {
      show(kind === "re-input-password" ? "wrongPassword" : "approval",
        kind === "re-input-password" ? "error" : "neutral");
      passwordForm.hidden = false;
      password.focus();
    } else if (kind === "error") {
      end(active ? "closed" : "unavailable");
    } else if (kind === "connecting") {
      show("connecting");
    }
  });
  connection.setDraw((frame) => video.drawFrame(frame));

  passwordForm.addEventListener("submit", (event) => {
    event.preventDefault();
    if (!password.value) return;
    connection.login(password.value);
    password.value = "";
  });
  disconnect.addEventListener("click", () => end("closed"));
  fullscreen.addEventListener("click", () => {
    const transition = document.fullscreenElement ? document.exitFullscreen() : root.requestFullscreen();
    void transition.catch(() => { fullscreen.hidden = true; });
  });
  display.addEventListener("change", () => {
    screen = Number(display.value);
    connection.switchDisplay(screen);
  });

  const pointer = (event: PointerEvent, type: number) => {
    if (!active) return;
    const geometry = displays[screen];
    if (!geometry) return;
    const rect = canvas.getBoundingClientRect();
    if (!rect.width || !rect.height) return;
    const scale = (position: number, start: number, shown: number, size: number) =>
      Math.round(Math.max(0, Math.min(1, (position - start) / shown)) * Math.max(0, size - 1));
    const x = geometry.x + scale(event.clientX, rect.left, rect.width, geometry.width);
    const y = geometry.y + scale(event.clientY, rect.top, rect.height, geometry.height);
    const modifiers = [event.altKey, event.ctrlKey, event.shiftKey, event.metaKey] as const;
    const button = event.button === 1 ? 4 : event.button === 2 ? 2 : 1;
    connection.inputMouse(0, x, y, ...modifiers);
    if (type) connection.inputMouse(type | (button << 3), x, y, ...modifiers);
  };
  canvas.addEventListener("contextmenu", (event) => event.preventDefault());
  canvas.addEventListener("pointermove", (event) => pointer(event, 0));
  canvas.addEventListener("pointerdown", (event) => {
    if (!active) return;
    event.preventDefault();
    canvas.focus();
    canvas.setPointerCapture(event.pointerId);
    pointer(event, 1);
  });
  canvas.addEventListener("pointerup", (event) => pointer(event, 2));
  canvas.addEventListener("pointercancel", (event) => pointer(event, 2));
  canvas.addEventListener("wheel", (event) => {
    if (!active) return;
    event.preventDefault();
    const step = (value: number) => value ? (value > 0 ? -1 : 1) : 0;
    connection.inputMouse(3, step(event.deltaX), step(event.deltaY));
  }, { passive: false });

  for (const type of ["keydown", "keyup"] as const) {
    canvas.addEventListener(type, (event) => {
      if (!active || event.isComposing) return;
      const name = controlKeys[event.key] ||
        (event.key.length === 1 || /^F([1-9]|1[0-2])$/.test(event.key) ? event.key : "");
      if (!name) return;
      event.preventDefault();
      if (type === "keydown") pressed.add(name);
      else pressed.delete(name);
      connection.inputKey(name, type === "keydown", false, event.altKey, event.ctrlKey, event.shiftKey, event.metaKey);
    });
  }
  canvas.addEventListener("blur", () => {
    if (!active) return;
    for (const name of pressed) connection.inputKey(name, false, false, false, false, false, false);
    pressed.clear();
  });
  canvas.addEventListener("paste", (event) => {
    if (!active) return;
    event.preventDefault();
    const value = event.clipboardData?.getData("text");
    if (value) connection.inputString(value);
  });
  const sendMobileText = (event: Event) => {
    const field = event.target as HTMLTextAreaElement;
    if (!active || (event as InputEvent).isComposing) return;
    if (field.value) connection.inputString(field.value);
    field.value = "";
  };
  textInput?.addEventListener("input", sendMobileText);
  textInput?.addEventListener("compositionend", sendMobileText);
  window.addEventListener("pagehide", () => end("closed"), { once: true });

  const start = async () => {
    await browser.init?.();
    await connection.start(guestLaunch.peer_id);
  };
  void start().catch(() => end("unavailable"));
}

if (guestMode) {
  const root = document.getElementById("guest-viewer");
  if (root) startViewer(root);
}
