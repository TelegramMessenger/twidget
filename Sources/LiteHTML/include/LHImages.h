#import <Cocoa/Cocoa.h>

NS_ASSUME_NONNULL_BEGIN

@interface LHImages : NSObject

+ (void)registerImage:(NSImage *)image name:(NSString *)name;
+ (void)setProvider:(nullable NSImage *_Nullable (^)(NSString *url))provider;
@property (class, readonly, nullable) NSImage *_Nullable (^provider)(NSString *url);
+ (void)forgetImageWithURL:(NSString *)url;
+ (void)registerDemoImages;

@end

NS_ASSUME_NONNULL_END
