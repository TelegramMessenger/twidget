#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

@interface LHImages : NSObject

+ (void)registerImage:(NSImage *)image name:(NSString *)name;
+ (void)setProvider:(nullable NSImage *_Nullable (^)(NSString *url))provider;
+ (void)forgetImageWithURL:(NSString *)url;
+ (void)registerDemoImages;

@end

NS_ASSUME_NONNULL_END
