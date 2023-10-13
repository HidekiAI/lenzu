#include <opencv2/opencv.hpp>
#include <iostream>
#include <vector>
#include <algorithm>
#include <iostream> // for cout/cin:
// Platform-specific headers
#ifdef _WIN32
#include <windows.h>
#else
#include <X11/Xlib.h>
#endif

using namespace std;


int main()
{
    // Common code that works on both Windows and Linux
    // ...

#ifdef _WIN32
    // Windows-specific code
    POINT cursorPos;
    // Get the cursor position (Windows-specific function?)
    GetCursorPos(&cursorPos);

    // Get the number of screens and their dimensions
    int numScreens = GetSystemMetrics(SM_CMONITORS);
    std::vector<RECT> screenDimensions(numScreens);

    // Populate screenDimensions with Windows-specific logic
    for (int i = 0; i < numScreens; i++)
    {
        screenDimensions[i].left = GetSystemMetrics(SM_XVIRTUALSCREEN) +
                                   i * GetSystemMetrics(SM_CXVIRTUALSCREEN);
        screenDimensions[i].top = GetSystemMetrics(SM_YVIRTUALSCREEN);
        screenDimensions[i].right =
            screenDimensions[i].left + GetSystemMetrics(SM_CXSCREEN);
        screenDimensions[i].bottom =
            screenDimensions[i].top + GetSystemMetrics(SM_CYSCREEN);
    }

    int screenNumber = 0; // Initialize to the primary screen (screen 0)

    // More Windows-specific code
    int screenNumber = 0; // Initialize to the primary screen (screen 0)
    for (int i = 0; i < numScreens; i++)
    {
        if (cursorPos.x >= screenDimensions[i].left &&
            cursorPos.x <= screenDimensions[i].right &&
            cursorPos.y >= screenDimensions[i].top &&
            cursorPos.y <= screenDimensions[i].bottom)
        {
            screenNumber = i;
            break;
        }
    }

    // Calculate the cropping region around the cursor
    int cropSize = 256;                                                                                                                                  // Default to 256
    int cropX = (((cursorPos.x - cropSize) > (screenDimensions[screenNumber].left)) ? (cursorPos.x - cropSize) : (screenDimensions[screenNumber].left)); // std::max(cursorX - cropSize, screenDimensions[screenNumber].left);
    int cropY = (((cursorPos.y - cropSize) > (screenDimensions[screenNumber].top)) ? (cursorPos.y - cropSize) : (screenDimensions[screenNumber].top));   // std::max(cursorY - cropSize, screenDimensions[screenNumber].top);

    // Make sure the crop region stays within the screen boundaries
    int cropWidth =
        (((cropSize * 2) < (screenDimensions[screenNumber].right - cropX)) ? (cropSize * 2) : (screenDimensions[screenNumber].right - cropX)); // std::min(cropSize * 2, screenDimensions[screenNumber].right - cropX);
    int cropHeight =
        (((cropSize * 2) < (screenDimensions[screenNumber].bottom - cropY)) ? (cropSize * 2) : (screenDimensions[screenNumber].bottom - cropY)); // std::min(cropSize * 2, screenDimensions[screenNumber].bottom - cropY);

    // Initialize OpenCV VideoCapture for screen capture (using screenNumber)
    cv::VideoCapture cap(screenNumber);

    while (true)
    {
        cv::Mat frame;
        cap >> frame;

        if (!frame.empty())
        {
            // Crop the frame to the specified region
            cv::Rect region_of_interest(cropX, cropY, cropWidth, cropHeight);
            cv::Mat cropped = frame(region_of_interest);

            // Display the cropped frame
            cv::imshow("Captured Window", cropped);

            if (cv::waitKey(10) == 27)
            {
                break; // Exit on ESC key.
            }
        }
    }
#else
    // Linux-specific code
    XWindowAttributes windowAttributes;
    Display *display = XOpenDisplay(NULL);
    if (display == NULL)
    {
        std::cerr << "Failed to open X display." << std::endl;
        return 1;
    }

    Window root = DefaultRootWindow(display);
    XGetWindowAttributes(display, root, &windowAttributes);

    // Get the number of screens and their dimensions
    int numScreens = ScreenCount(display); // Is this 1-based?
    std::vector<XWindowAttributes> screenAttributes(numScreens, windowAttributes);

    // You can adjust screenAttributes as needed for multi-screen setups on Linux.
    int screenNumber = XScreenNumberOfScreen(windowAttributes.screen);
    //std::cout << "Cursor is on screen " << screenNumber << std::endl;
   Window rootWindow = RootWindow(display, screenNumber);

    int cursorPosX, cursorPosY;
    unsigned int mask;
    if (!XQueryPointer(display, rootWindow, &rootWindow, &rootWindow, &cursorPosX, &cursorPosY, &cursorPosX, &cursorPosY, &mask))
    {
        std::cerr << "Failed to query pointer." << std::endl;
    }
    // Calculate the cropping region around the cursor
    int cropSize = 256;                                                                                                                                  // Default to 256
    int cropX = (((cursorPosX - cropSize) > (screenAttributes[screenNumber].x)) ? (cursorPosX - cropSize) : (screenAttributes[screenNumber].x)); // std::max(cursorX - cropSize, screenDimensions[screenNumber].left);
    int cropY = (((cursorPosY - cropSize) > (screenAttributes[screenNumber].y)) ? (cursorPosY - cropSize) : (screenAttributes[screenNumber].y));   // std::max(cursorY - cropSize, screenDimensions[screenNumber].top);

    // Make sure the crop region stays within the screen boundaries
    int cropWidth =
        (((cropSize * 2) < (screenAttributes[screenNumber].width - cropX)) ? (cropSize * 2) : (screenAttributes[screenNumber].width - cropX)); // std::min(cropSize * 2, screenDimensions[screenNumber].right - cropX);
    int cropHeight =
        (((cropSize * 2) < (screenAttributes[screenNumber].height - cropY)) ? (cropSize * 2) : (screenAttributes[screenNumber].height - cropY)); // std::min(cropSize * 2, screenDimensions[screenNumber].bottom - cropY);

    // Initialize OpenCV VideoCapture for screen capture (using screenNumber)
    cv::VideoCapture cap(screenNumber);

    cout << "cropX: " << cropX << endl;
    cout << "cropY: " << cropY << endl;
    cout << "cropWidth: " << cropWidth << endl;
    cout << "cropHeight: " << cropHeight << endl;
    cout << "screenAttributes[screenNumber].width: " << screenAttributes[screenNumber].width << endl;
    cout << "screenAttributes[screenNumber].height: " << screenAttributes[screenNumber].height << endl;
    cout << "screenAttributes[screenNumber].x: " << screenAttributes[screenNumber].x << endl;
    cout << "screenAttributes[screenNumber].y: " << screenAttributes[screenNumber].y << endl;
    cout << "screenAttributes[screenNumber].width - cropX: " << screenAttributes[screenNumber].width - cropX << endl;
    cout << "screenAttributes[screenNumber].height - cropY: " << screenAttributes[screenNumber].height - cropY << endl;
    cout << "Hit Esc to exit" << endl;
    while (true)
    {
        cv::Mat frame;
        cap >> frame;

        if (!frame.empty())
        {
            cout << "Cropping image" << endl;
            // Crop the frame to the specified region
            cv::Rect region_of_interest(cropX, cropY, cropWidth, cropHeight);
            cv::Mat cropped = frame(region_of_interest);

            // Display the cropped frame
            cv::imshow("Captured Window", cropped);
            if (cv::waitKey(10) == 27)  // wait for 10 ms for a key to be pressed
            {
                break; // Exit on ESC key.
            }
        }
    }

    XCloseDisplay(display);
#endif

    // Common code that works on both Windows and Linux
    // ...

    return 0;
}
