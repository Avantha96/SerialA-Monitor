// SerialX high-speed test stream — Arduino Nano / Uno (ATmega328P @ 16 MHz)
//
// Streams numbered, checksummed text lines as fast as the UART allows, so a PC tool
// can measure real throughput and detect lost or corrupted data.
//
// Line format (56 bytes including \r\n):
//   $SSSSSSSS,TTTTTTTT,<32 payload chars>*CC\r\n
//     SSSSSSSS  sequence number, hex, +1 per line (a gap = lost lines)
//     TTTTTTTT  micros() on the board when the line was built, hex
//     payload   'A'..'Z' rotating, derived from the sequence number
//     CC        XOR of every character between '$' and '*', hex (like NMEA)
// Info/reply lines start with '#'.
//
// Commands (send a line ending in '\n' at the current baud rate):
//   G            start streaming          S   stop streaming
//   R            reset sequence to 0      ?   print status
//   B<baud>      switch baud rate, e.g. B2000000 (reply is sent at the old rate first)
//   D<us>        delay between lines in microseconds (0 = as fast as possible)
//
// Baud rates that are exact on a 16 MHz AVR: 250000, 500000, 1000000, 2000000.
// 115200 works but is ~2% off nominal.

#include <Arduino.h>

static const uint32_t DEFAULT_BAUD = 1000000;
static const char HEX_DIGITS[] = "0123456789ABCDEF";

static uint32_t baud = DEFAULT_BAUD;
static uint32_t seq = 0;
static uint8_t payloadStart = 0;   // == seq % 26
static uint32_t lineDelayUs = 0;
static bool streaming = true;
static char cmd[16];
static uint8_t cmdLen = 0;

// Transmit by polling the data register directly. HardwareSerial's interrupt-driven
// TX costs more CPU per byte than one byte-time at 2 Mbaud; polling keeps the line full.
static inline void txByte(uint8_t c) {
  while (!(UCSR0A & _BV(UDRE0))) {}
  UDR0 = c;
}
static void txBuf(const char *s, uint8_t n) {
  for (uint8_t i = 0; i < n; i++) txByte((uint8_t)s[i]);
}
static void txStr(const char *s) {
  while (*s) txByte((uint8_t)*s++);
}
static void txDec(uint32_t v) {
  char b[11];
  uint8_t i = sizeof(b);
  b[--i] = 0;
  do { b[--i] = '0' + (v % 10); v /= 10; } while (v);
  txStr(&b[i]);
}
// Constant byte shifts only: a variable-count 32-bit shift is a slow loop on AVR.
static inline char *putHex8(char *p, uint8_t b) {
  *p++ = HEX_DIGITS[b >> 4];
  *p++ = HEX_DIGITS[b & 0xF];
  return p;
}
static char *putHex32(char *p, uint32_t v) {
  p = putHex8(p, (uint8_t)(v >> 24));
  p = putHex8(p, (uint8_t)(v >> 16));
  p = putHex8(p, (uint8_t)(v >> 8));
  return putHex8(p, (uint8_t)v);
}

static void waitTxDone() {
  // Wait until the data register is empty, then long enough for the byte still in the
  // shift register to leave the pin: 2 byte-times at 9600 baud is about 2.1 ms.
  while (!(UCSR0A & _BV(UDRE0))) {}
  delay(3);
}

static void status() {
  txStr("#SerialX-HS v1 baud=");
  txDec(baud);
  txStr(" seq=");
  txDec(seq);
  txStr(" delay_us=");
  txDec(lineDelayUs);
  txStr(streaming ? " streaming=1\r\n" : " streaming=0\r\n");
}

static void setBaud(uint32_t newBaud) {
  txStr("#OK B");
  txDec(newBaud);
  txStr("\r\n");
  waitTxDone();
  Serial.end();
  baud = newBaud;
  Serial.begin(baud);
}

static void runCommand() {
  cmd[cmdLen] = 0;
  char c = cmd[0];
  uint32_t arg = strtoul(cmd + 1, nullptr, 10);
  switch (c) {
    case 'G': case 'g': streaming = true; break;
    case 'S': case 's': streaming = false; txStr("#OK S\r\n"); break;
    case 'R': case 'r': seq = 0; payloadStart = 0; txStr("#OK R\r\n"); break;
    case 'D': case 'd': lineDelayUs = arg; txStr("#OK D\r\n"); break;
    case '?': status(); break;
    case 'B': case 'b':
      if (arg >= 9600 && arg <= 2000000) setBaud(arg);
      else txStr("#ERR baud must be 9600..2000000\r\n");
      break;
    default:
      if (cmdLen) txStr("#ERR unknown command\r\n");
  }
  cmdLen = 0;
}

static void pollCommands() {
  while (Serial.available()) {
    char c = (char)Serial.read();
    if (c == '\n' || c == '\r') {
      if (cmdLen) runCommand();
    } else if (cmdLen < sizeof(cmd) - 1) {
      cmd[cmdLen++] = c;
    }
  }
}

static void sendLine() {
  char buf[64];
  char *p = buf;
  *p++ = '$';
  p = putHex32(p, seq);
  *p++ = ',';
  p = putHex32(p, micros());
  *p++ = ',';
  // Payload starts at 'A' + seq % 26, tracked incrementally: a 32-bit modulo
  // is a ~40 µs software division on AVR.
  char ch = 'A' + payloadStart;
  for (uint8_t i = 0; i < 32; i++) {
    *p++ = ch;
    ch = (ch == 'Z') ? 'A' : ch + 1;
  }
  uint8_t x = 0;
  for (char *q = buf + 1; q < p; q++) x ^= (uint8_t)*q;
  *p++ = '*';
  p = putHex8(p, x);
  *p++ = '\r';
  *p++ = '\n';
  txBuf(buf, (uint8_t)(p - buf));
  seq++;
  if (++payloadStart == 26) payloadStart = 0;
}

void setup() {
  Serial.begin(baud);
  pinMode(LED_BUILTIN, OUTPUT);
  txStr("\r\n");
  status();
}

void loop() {
  pollCommands();
  if (streaming) {
    sendLine();
    if (lineDelayUs) delayMicroseconds(lineDelayUs > 16000 ? 16000 : lineDelayUs);
    if (lineDelayUs > 16000) delay((lineDelayUs - 16000) / 1000);
    if ((seq & 0x3FF) == 0) PINB = _BV(PINB5);  // toggle LED (D13) every 1024 lines
  }
}
