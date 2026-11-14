// Lets Finder hand files and folders to Slate ("Open With", double-click,
// dropping on the Dock icon).
//
// macOS does not pass those on the command line. It sends the running app
// an "open" event, which AppKit turns into a call to the application
// delegate's `application:openURLs:`. The windowing library (winit) owns
// that delegate and does not implement the method, so AppKit reports that
// the app "cannot open files in this format". This file adds the method
// to winit's delegate and forwards each path to Rust.

#import <Cocoa/Cocoa.h>
#import <objc/runtime.h>

typedef void (*SlateOpenCallback)(const char *path);
static SlateOpenCallback slate_callback = NULL;

static void slate_open_urls(id self, SEL _cmd, NSApplication *app, NSArray<NSURL *> *urls) {
    (void)self;
    (void)_cmd;
    (void)app;
    if (slate_callback == NULL) {
        return;
    }
    for (NSURL *url in urls) {
        if ([url isFileURL] && [url path] != nil) {
            slate_callback([[url path] fileSystemRepresentation]);
        }
    }
}

@interface SlateOpenHook : NSObject <NSApplicationDelegate>
@end

@implementation SlateOpenHook

// Runs once, just before the app finishes launching: late enough that the
// delegate exists, early enough to catch the file the app was launched with.
- (void)attach:(NSNotification *)note {
    (void)note;
    NSApplication *app = [NSApplication sharedApplication];
    id<NSApplicationDelegate> delegate = [app delegate];
    if (delegate == nil) {
        [app setDelegate:self];
        return;
    }
    class_replaceMethod([(id)delegate class], @selector(application:openURLs:), (IMP)slate_open_urls, "v@:@@");
    // Set the delegate again so AppKit notices the method it now has.
    [app setDelegate:nil];
    [app setDelegate:delegate];
}

- (void)application:(NSApplication *)application openURLs:(NSArray<NSURL *> *)urls {
    slate_open_urls(self, _cmd, application, urls);
}

@end

static SlateOpenHook *slate_hook = nil;

void slate_install_open_handler(SlateOpenCallback callback) {
    slate_callback = callback;
    slate_hook = [[SlateOpenHook alloc] init];
    [[NSNotificationCenter defaultCenter] addObserver:slate_hook
                                             selector:@selector(attach:)
                                                 name:NSApplicationWillFinishLaunchingNotification
                                               object:nil];
}
