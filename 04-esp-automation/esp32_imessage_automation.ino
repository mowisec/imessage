#include <BleKeyboard.h>

BleKeyboard bleKeyboard("ESP32-BLE-Keyboard", "ESP32", 100);

// ================= CONFIG =================
static const unsigned long DEFAULT_INTERVAL = 600000; // 10 minutes
static const unsigned long SERIAL_TIMEOUT   = 60000;  // 1 minute
static const char PASSCODE[] = "123456";              // Screen unlock code
// ==========================================

// Timing
unsigned long sleepTime      = 0;
unsigned long bootTime       = 0;
unsigned long lastPromptTime = 0;

// ================= FUNCTIONS =================

void lockScreen() {
  bleKeyboard.press(KEY_LEFT_CTRL);
  bleKeyboard.press(KEY_LEFT_GUI);
  bleKeyboard.print("q");
  bleKeyboard.releaseAll();
}

void unlockScreen() {
  bleKeyboard.print(" ");
  delay(200);
  bleKeyboard.press(KEY_RETURN);
  bleKeyboard.releaseAll();
  delay(500);
  bleKeyboard.print(PASSCODE);
}

void openApp(const char* appName) {
  Serial.print("Opening app: ");
  Serial.println(appName);

  // Open launcher / search
  bleKeyboard.write(KEY_MEDIA_WWW_SEARCH);
  delay(1000);

  // Type app name
  bleKeyboard.print(appName);
  delay(500);

  // Launch app
  bleKeyboard.write(KEY_RETURN);
}

void openHome() {
  //bleKeyboard.write(KEY_ESC);
  bleKeyboard.press(KEY_LEFT_GUI);
  bleKeyboard.print("h");
  bleKeyboard.releaseAll();
  delay(1000);
}

// Sleep function that accounts for execution time
void sleepRemaining(unsigned long startTime) {
  unsigned long elapsed = millis() - startTime;
  if (elapsed < sleepTime) {
    delay(sleepTime - elapsed);
  } else {
    Serial.println("⚠️ Phase overran sleepTime!");
  }
}

void readSerialDuration() {
  if (Serial.available()) {
    sleepTime = Serial.parseInt();
    // Clear leftover characters in serial buffer
    while (Serial.available()) Serial.read();
    if (sleepTime > 0) {
      Serial.print("Sleep time set to: ");
      Serial.println(sleepTime);
    }
  }
}

void applyDefaultIfTimeout() {
  if (sleepTime == 0 && millis() - bootTime > SERIAL_TIMEOUT) {
    sleepTime = DEFAULT_INTERVAL;
    Serial.println("No input received. Defaulting to 10 minutes.");
  }
}

void runPayload() {
  Serial.println("Starting payload...");

  unsigned long cycleStart;

  // ---------- Screen ON ----------
  cycleStart = millis();
  Serial.print("Keeping screen on for ");
  Serial.print(sleepTime);
  Serial.println(" ms");
  unlockScreen();
  sleepRemaining(cycleStart);

  // ---------- Screen OFF ----------
  cycleStart = millis();
  Serial.print("Keeping screen off for ");
  Serial.print(sleepTime);
  Serial.println(" ms");
  lockScreen();
  sleepRemaining(cycleStart);

  // ---------- App OPEN ----------
  cycleStart = millis();
  Serial.print("Keeping app open for ");
  Serial.print(sleepTime);
  Serial.println(" ms");
  unlockScreen();
  openApp("messages");
  sleepRemaining(cycleStart);

  // ---------- Home + Lock ----------
  cycleStart = millis();
  Serial.println("Going Home...");
  openHome();
  lockScreen();
  Serial.print("Keeping screen off for ");
  Serial.print(sleepTime);
  Serial.println(" ms");
  sleepRemaining(cycleStart);

  Serial.println("Full cycle done.\n");
}

// ================= SETUP & LOOP =================

void setup() {
  Serial.begin(115200);
  bootTime = millis();

  Serial.println("\n=== BLE Keyboard Started ===");
  Serial.println("Enter sleep time in milliseconds:");

  bleKeyboard.begin();
}

void loop() {

  // Wait for sleep time input
  if (sleepTime == 0) {
    if (millis() - lastPromptTime > 5000) {
      Serial.println("Waiting for sleep time input...");
      lastPromptTime = millis();
    }

    readSerialDuration();
    applyDefaultIfTimeout();
    return;
  }

  while (bleKeyboard.isConnected()) {
    runPayload();
  }
  delay(1000);
}
