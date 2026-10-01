import { execFileSync } from "node:child_process";
import { mkdir, writeFile } from "node:fs/promises";
import { join, resolve } from "node:path";

// Failure modes: backup remains enabled after manifest merge; USB filters still
// claim unrelated devices; native pinch scales the whole page; app closes.
const output = resolve(process.argv[2] ?? ".playwright-mcp/android-platform");
const device = process.env.CULLANT_ADB_DEVICE ?? "100.89.241.106:45353";
const adb = process.env.CULLANT_ADB ?? "C:/Android/sdk/platform-tools/adb.exe";
const port = process.env.CULLANT_CDP_PORT ?? "9223";
const apkOnly = process.env.CULLANT_APK_ONLY === "1";
const apk = resolve(process.env.CULLANT_APK ?? "src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk");
const aapt = process.env.CULLANT_AAPT ?? "C:/Android/sdk/build-tools/36.0.0/aapt2.exe";
const results = [];
const evidence = {};
let socket;
await mkdir(output, { recursive: true });

function deviceCommand(args) {
  return execFileSync(adb, ["-s", device, ...args], { encoding: "utf8", maxBuffer: 8 * 1024 * 1024 });
}
function check(name, condition, details) {
  results.push({ name, status: condition ? "PASS" : "FAIL", details });
  console.log(`${condition ? "PASS" : "FAIL"}: ${name}`);
}

try {
  if (apkOnly) {
    const xml = file => execFileSync(aapt, ["dump", "xmltree", "--file", file, apk], { encoding: "utf8" });
    const manifest = xml("AndroidManifest.xml");
    const filters = xml("res/xml/usb_device_filter.xml");
    const backups = xml("res/xml/data_extraction_rules.xml");
    evidence.apk = apk;
    evidence.manifest = manifest;
    evidence.usbFilters = filters;
    evidence.cloudBackupRules = backups;
    check("APK disables automatic app backup", /:allowBackup[^\n]*=false/.test(manifest), manifest.match(/[^\n]*:allowBackup[^\n]*/)?.[0]);
    check("APK disables full backup on older Android", /:fullBackupContent[^\n]*=false/.test(manifest), manifest.match(/[^\n]*:fullBackupContent[^\n]*/)?.[0]);
    check("APK applies Android 12 cloud rules", /:dataExtractionRules[^\n]*=@0x/.test(manifest) && ["root", "file", "database", "sharedpref", "external", "device_root", "device_file", "device_database", "device_sharedpref"].every(domain => backups.includes(`domain="${domain}"`)), backups);
    check("APK uses camera, storage, and vendor USB filters", [6, 8, 255].every(value => new RegExp(`class=${value}(?:\\s|$)`).test(filters)) && (filters.match(/E: usb-device/g) ?? []).length === 3, filters);
    results.push({ name: "Native pinch on a connected device", status: "NOT_RUN", details: "APK verification only. Run without CULLANT_APK_ONLY on an unlocked debug device." });
  } else {
  const pkg = deviceCommand(["shell", "dumpsys", "package", "org.cullant.app"]);
  const flags = pkg.split("\n").find(line => /^\s*pkgFlags=/.test(line));
  evidence.packageFlags = flags;
  check("Installed app disables cloud backup", !!flags && !flags.includes("ALLOW_BACKUP"), flags);

  const usb = deviceCommand(["shell", "dumpsys", "usb"]);
  const activity = usb.match(/activity=\{\s*package_name=org\.cullant\.app[\s\S]*?(?=\s*\{\s*activity=|\s*accessory_attached_activities=|\s*profile_group_settings=)/)?.[0] ?? "";
  evidence.cullantUsbFilters = activity;
  check("Cullant handles storage, PTP, and vendor USB interfaces", [6, 8, 255].every(value => new RegExp(`\\n\\s+class=${value}\\s`).test(activity)), activity);
  check("Cullant does not claim every USB device", !/\n\s+class=-1\s/.test(activity), activity);
  evidence.usbListeners = deviceCommand(["shell", "cmd", "package", "query-activities", "--brief", "-a", "android.hardware.usb.action.USB_DEVICE_ATTACHED"]);

  const pages = await (await fetch(`http://127.0.0.1:${port}/json/list`)).json();
  const page = pages.find(target => target.type === "page" && /tauri\.localhost|localhost:1420/.test(target.url));
  if (!page) throw new Error("Start the debug Android app and forward its WebView socket to the CDP port.");
  socket = new WebSocket(page.webSocketDebuggerUrl);
  await new Promise((resolve, reject) => { socket.onopen = resolve; socket.onerror = reject; });
  let nextId = 0;
  const pending = new Map();
  socket.onmessage = ({ data }) => {
    const reply = JSON.parse(data);
    const request = pending.get(reply.id);
    if (!request) return;
    pending.delete(reply.id);
    if (reply.error) request.reject(new Error(reply.error.message));
    else request.resolve(reply.result);
  };
  function send(method, params = {}) {
    return new Promise((resolve, reject) => {
      const id = ++nextId;
      const timer = setTimeout(() => { pending.delete(id); reject(new Error(`CDP timeout: ${method}`)); }, 15000);
      pending.set(id, { resolve: value => { clearTimeout(timer); resolve(value); }, reject: error => { clearTimeout(timer); reject(error); } });
      socket.send(JSON.stringify({ id, method, params }));
    });
  }
  const viewport = async () => (await send("Runtime.evaluate", { expression: "({scale:visualViewport.scale,width:innerWidth,height:innerHeight})", returnByValue: true })).result.value;
  const before = await viewport();
  await send("Input.synthesizePinchGesture", { x: before.width / 2, y: 40, scaleFactor: 1.8, gestureSourceType: "touch" });
  const after = await viewport();
  evidence.viewport = { before, after };
  check("Native pinch keeps the application viewport fixed", after.scale === before.scale && after.width === before.width, evidence.viewport);
  const focus = deviceCommand(["shell", "dumpsys", "window"]).split("\n").find(line => line.includes("mCurrentFocus="));
  check("Cullant stays in the foreground", focus?.includes("org.cullant.app"), focus);
  if (!focus?.includes("org.cullant.app")) throw new Error("Leave Cullant in the foreground during this test.");
  const shot = execFileSync(adb, ["-s", device, "exec-out", "screencap", "-p"], { maxBuffer: 16 * 1024 * 1024 });
  await writeFile(join(output, "after-native-pinch.png"), shot);
  }
} catch (error) {
  check("Device verification completes", false, error.stack ?? error.message);
} finally {
  socket?.close();
  await writeFile(join(output, "results.json"), JSON.stringify({ date: new Date().toISOString(), device, results, evidence, hardwareLimit: "The script does not simulate physical USB attachment or test the Android app chooser." }, null, 2));
}
if (results.some(result => result.status === "FAIL")) process.exitCode = 1;
