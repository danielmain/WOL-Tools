#import <AuthenticationServices/AuthenticationServices.h>
#import <UIKit/UIKit.h>
#import <Foundation/Foundation.h>

typedef void (*WolOAuthCompletion)(void *ctx, const char *callback_url, const char *error);

@interface WolAuthContextProvider : NSObject <ASWebAuthenticationPresentationContextProviding>
@end

@implementation WolAuthContextProvider
- (ASPresentationAnchor)presentationAnchorForWebAuthenticationSession:(ASWebAuthenticationSession *)session {
    (void)session;
    for (UIScene *scene in UIApplication.sharedApplication.connectedScenes) {
        if ([scene isKindOfClass:[UIWindowScene class]] &&
            scene.activationState == UISceneActivationStateForegroundActive) {
            UIWindowScene *windowScene = (UIWindowScene *)scene;
            for (UIWindow *window in windowScene.windows) {
                if (window.isKeyWindow) {
                    return window;
                }
            }
            if (windowScene.windows.count > 0) {
                return windowScene.windows.firstObject;
            }
        }
    }
    return [[UIWindow alloc] init];
}
@end

void wol_ios_start_oauth_session(
    const char *auth_url_cstr,
    const char *callback_scheme_cstr,
    void *ctx,
    WolOAuthCompletion completion
) {
    NSString *urlString = auth_url_cstr ? [NSString stringWithUTF8String:auth_url_cstr] : @"";
    NSString *scheme = callback_scheme_cstr ? [NSString stringWithUTF8String:callback_scheme_cstr] : @"";
    NSURL *url = [NSURL URLWithString:urlString];

    if (!url || scheme.length == 0) {
        if (completion) {
            completion(ctx, NULL, "Invalid OAuth URL or callback scheme.");
        }
        return;
    }

    dispatch_async(dispatch_get_main_queue(), ^{
        WolAuthContextProvider *provider = [[WolAuthContextProvider alloc] init];
        __block ASWebAuthenticationSession *session = nil;
        __block WolAuthContextProvider *retainedProvider = provider;

        session = [[ASWebAuthenticationSession alloc]
            initWithURL:url
            callbackURLScheme:scheme
            completionHandler:^(NSURL * _Nullable callbackURL, NSError * _Nullable error) {
                if (completion) {
                    if (callbackURL) {
                        completion(ctx, callbackURL.absoluteString.UTF8String, NULL);
                    } else {
                        const char *errMsg = error ? error.localizedDescription.UTF8String : "Sign-in cancelled.";
                        completion(ctx, NULL, errMsg);
                    }
                }
                session = nil;
                retainedProvider = nil;
            }];

        session.presentationContextProvider = provider;
        session.prefersEphemeralWebBrowserSession = NO;

        if (![session start]) {
            if (completion) {
                completion(ctx, NULL, "Failed to start ASWebAuthenticationSession.");
            }
            session = nil;
            retainedProvider = nil;
        }
    });
}
