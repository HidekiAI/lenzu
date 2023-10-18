// dc_demo_win.cpp : Defines the entry point for the application.
#include "dc_demo_win.h"

using namespace std;

LRESULT CALLBACK WindowProc(HWND hwnd, UINT uMsg, WPARAM wParam,
                            LPARAM lParam) {
  if (uMsg == WM_KEYDOWN) {
    if (wParam == VK_ESCAPE) {
      PostQuitMessage(0);
    }
  } else if (uMsg == WM_DESTROY) {
    PostQuitMessage(0);
  }
  return DefWindowProc(hwnd, uMsg, wParam, lParam);
}

int main() {
  // Register window class
  const char CLASS_NAME[] = "Sample Window Class";
  WNDCLASS wc = {};
  wc.lpfnWndProc = WindowProc;
  wc.hInstance = GetModuleHandle(NULL);
  wc.lpszClassName = CLASS_NAME;
  RegisterClass(&wc);

  // Create window
  HWND hwnd =
      CreateWindowEx(0, CLASS_NAME, "Test Window", WS_OVERLAPPEDWINDOW,
                     CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT, CW_USEDEFAULT,
                     NULL, NULL, GetModuleHandle(NULL), NULL);

  if (hwnd == NULL) {
    return 0;
  }

  ShowWindow(hwnd, SW_SHOWDEFAULT);

  // Message loop
  MSG msg = {};
  while (GetMessage(&msg, NULL, 0, 0)) {
    TranslateMessage(&msg);
    DispatchMessage(&msg);

    POINT cursorPos;
    GetCursorPos(&cursorPos);

    // Get the monitor where the mouse cursor is located
    HMONITOR hMonitor = MonitorFromPoint(cursorPos, MONITOR_DEFAULTTONEAREST);
    MONITORINFO monitorInfo;
    monitorInfo.cbSize = sizeof(MONITORINFO);
    GetMonitorInfo(hMonitor, &monitorInfo);

    // Calculate the rectangle to capture
    RECT captureRect;
    captureRect.left = max(monitorInfo.rcMonitor.left, cursorPos.x - 256);
    captureRect.top = max(monitorInfo.rcMonitor.top, cursorPos.y - 256);
    captureRect.right = min(monitorInfo.rcMonitor.right, cursorPos.x + 256);
    captureRect.bottom = min(monitorInfo.rcMonitor.bottom, cursorPos.y + 256);

    // Create a bitmap
    HDC hScreen = GetDC(NULL);
    HDC hDC = CreateCompatibleDC(hScreen);
    HBITMAP hBitmap =
        CreateCompatibleBitmap(hScreen, captureRect.right - captureRect.left,
                               captureRect.bottom - captureRect.top);
    HGDIOBJ old_obj = SelectObject(hDC, hBitmap);
    BOOL bRet = BitBlt(hDC, 0, 0, captureRect.right - captureRect.left,
                       captureRect.bottom - captureRect.top, hScreen,
                       captureRect.left, captureRect.top, SRCCOPY);

    // Draw the bitmap on the window
    PAINTSTRUCT ps;
    HDC hdc = BeginPaint(hwnd, &ps);

    // Scale the bitmap
    StretchBlt(hdc, 0, 0, (captureRect.right - captureRect.left) * 2,
               (captureRect.bottom - captureRect.top) * 2, hDC, 0, 0,
               captureRect.right - captureRect.left,
               captureRect.bottom - captureRect.top, SRCCOPY);

    // Draw the cursor on the window
    CURSORINFO cursorInfo;
    cursorInfo.cbSize = sizeof(CURSORINFO);
    GetCursorInfo(&cursorInfo);
    DrawIcon(hdc, (cursorInfo.ptScreenPos.x - captureRect.left) * 2,
             (cursorInfo.ptScreenPos.y - captureRect.top) * 2,
             cursorInfo.hCursor);

    EndPaint(hwnd, &ps);

    InvalidateRect(hwnd, NULL,
                   FALSE); // Add this line to keep refreshing the window

    // Clean up
    SelectObject(hDC, old_obj);
    DeleteDC(hDC);
    ReleaseDC(NULL, hScreen);
    DeleteObject(hBitmap);
  }

  return 0;
}
