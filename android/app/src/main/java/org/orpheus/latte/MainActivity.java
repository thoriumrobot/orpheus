package org.orpheus.latte;

// MainActivity — the whole Android app.
//
// Orpheus is one static binary with everything inside it: the Latte language, the Loom VM,
// the interpreter and JIT, the .lat libraries, and (since src/site.rs) the GUI's pages.
// So the app has exactly two jobs:
//
//   1. RUN the binary. Android has forbidden executing files from an app's writable data
//      directory since API 29 (W^X). The one directory that still holds executable files is
//      nativeLibraryDir — the place the installer extracts everything the APK ships under
//      lib/<abi>/. So we ship the binary as `lib/arm64-v8a/liblatte.so`: it is not a shared
//      object at all, just our ELF executable wearing the name the packager requires. The
//      installer extracts it with the execute bit set, and we exec it from there. Nothing is
//      ever written to a writable-executable location, so this is policy-compliant, not a
//      workaround — and it needs no root, no Termux, and no sideloaded toolchain.
//
//   2. SHOW the GUI. The binary serves it on 127.0.0.1; we point a WebView at the URL it
//      prints. `latte android` binds loopback only, so nothing on the Wi-Fi can reach it.
//
// There is no rustc on a phone, so Anvil never compiles: the interpreter + JIT are the
// engine, and (src/loom.rs) the step budget is raised accordingly so the heavy finance and
// ML models actually finish instead of dying with OutOfFuel.

import android.app.Activity;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.webkit.WebSettings;
import android.webkit.WebView;
import android.webkit.WebViewClient;
import android.widget.TextView;

import java.io.BufferedReader;
import java.io.File;
import java.io.InputStreamReader;

public class MainActivity extends Activity {
    private Process latte;
    private WebView web;
    private TextView status;
    private final Handler ui = new Handler(Looper.getMainLooper());

    @Override
    protected void onCreate(Bundle saved) {
        super.onCreate(saved);
        status = new TextView(this);
        status.setPadding(48, 96, 48, 48);
        status.setText("Starting Orpheus…");
        setContentView(status);

        // Start the engine off the UI thread; swap in the WebView when it prints its URL.
        new Thread(this::startEngine).start();
    }

    private void startEngine() {
        try {
            // The binary, extracted by the installer with the execute bit set.
            String exe = getApplicationInfo().nativeLibraryDir + "/liblatte.so";
            // App-private storage: HOME governs every cache Orpheus keeps (the market
            // series, the news wire, the Anvil program cache), so this keeps all of it
            // inside the sandbox and nothing needs any permission.
            File home = new File(getFilesDir(), "orpheus");
            //noinspection ResultOfMethodCallIgnored
            home.mkdirs();

            ProcessBuilder pb = new ProcessBuilder(exe, "android", "--home", home.getAbsolutePath());
            pb.environment().put("HOME", home.getAbsolutePath());
            pb.environment().put("TMPDIR", getCacheDir().getAbsolutePath());
            pb.redirectErrorStream(true);
            latte = pb.start();

            // `latte android` prints exactly one line of the form `ORPHEUS_URL <url>`.
            BufferedReader out = new BufferedReader(new InputStreamReader(latte.getInputStream()));
            String line;
            while ((line = out.readLine()) != null) {
                final String l = line;
                if (l.startsWith("ORPHEUS_URL ")) {
                    final String url = l.substring("ORPHEUS_URL ".length()).trim();
                    ui.post(() -> showGui(url));
                } else {
                    ui.post(() -> status.append("\n" + l));
                }
            }
        } catch (Exception e) {
            final String msg = e.toString();
            ui.post(() -> status.setText("Could not start Orpheus:\n" + msg));
        }
    }

    private void showGui(String url) {
        web = new WebView(this);
        WebSettings s = web.getSettings();
        s.setJavaScriptEnabled(true);   // the GUI's tools are JS-driven
        s.setDomStorageEnabled(true);
        // The pages come from our own loopback server; keep the WebView inside it.
        web.setWebViewClient(new WebViewClient());
        setContentView(web);
        web.loadUrl(url);
    }

    @Override
    public void onBackPressed() {
        if (web != null && web.canGoBack()) {
            web.goBack();
        } else {
            super.onBackPressed();
        }
    }

    @Override
    protected void onDestroy() {
        super.onDestroy();
        if (latte != null) {
            latte.destroy();   // the node and its threads exit with the process
        }
    }
}
