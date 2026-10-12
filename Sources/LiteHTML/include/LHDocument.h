#import <Cocoa/Cocoa.h>
#import "LHTextFragment.h"

NS_ASSUME_NONNULL_BEGIN

@class LHGPURenderer;

@interface LHDisplayList : NSObject
@property (readonly) BOOL gpuSupported;

- (instancetype)init NS_UNAVAILABLE;

- (NSArray<NSValue *> *)changedRectsSince:(LHDisplayList *)previous NS_SWIFT_NAME(changedRects(since:));
- (CGFloat)unchangedBottomSince:(LHDisplayList *)previous previousHeight:(CGFloat)previousHeight height:(CGFloat)height below:(CGFloat)top NS_SWIFT_NAME(unchangedBottom(since:previousHeight:height:below:));

@end

@interface LHDocument : NSObject

- (instancetype)initWithHTML:(NSString *)html css:(nullable NSString *)css NS_DESIGNATED_INITIALIZER;
/// Builds the next frame of `previous`. When only inline `style` values and text changed, the previous
/// document's tree is patched in place and handed over (much cheaper than parsing); `previous` then
/// rebuilds itself from its own HTML if it is used again. Otherwise this parses `html` from scratch.
- (instancetype)initWithHTML:(NSString *)html css:(nullable NSString *)css previous:(nullable LHDocument *)previous NS_DESIGNATED_INITIALIZER;
/// Whether this document was made by patching the previous one.
@property (readonly) BOOL reusedPrevious;
- (instancetype)init NS_UNAVAILABLE;

@property (nonatomic, readonly, copy) NSString *html;
@property (nonatomic) BOOL clipCulling;
@property (nonatomic, nullable) CGColorSpaceRef colorSpace;

- (CGSize)layoutWithMaxWidth:(CGFloat)maxWidth;
- (CGSize)layoutWithMaxWidth:(CGFloat)maxWidth preferredWidth:(CGFloat)preferredWidth NS_SWIFT_NAME(layout(maxWidth:preferredWidth:));
@property (readonly) CGSize size;

- (void)drawInContext:(CGContextRef)ctx origin:(CGPoint)origin clip:(CGRect)clip;
- (nullable CGImageRef)copyImageWithMaxWidth:(CGFloat)maxWidth rect:(CGRect)rect scale:(CGFloat)scale CF_RETURNS_RETAINED NS_SWIFT_NAME(copyImage(maxWidth:rect:scale:));
- (nullable CGImageRef)copyImageWithMaxWidth:(CGFloat)maxWidth rect:(CGRect)rect scale:(CGFloat)scale excluding:(NSSet<NSString *> *)values attribute:(NSString *)attribute CF_RETURNS_RETAINED NS_SWIFT_NAME(copyImage(maxWidth:rect:scale:excluding:attribute:));
- (nullable CGImageRef)copyImageWithMaxWidth:(CGFloat)maxWidth rect:(CGRect)rect scale:(CGFloat)scale base:(CGImageRef)base dirty:(CGRect)dirty CF_RETURNS_RETAINED NS_SWIFT_NAME(copyImage(maxWidth:rect:scale:base:dirty:));

- (LHDisplayList *)displayListWithMaxWidth:(CGFloat)maxWidth NS_SWIFT_NAME(displayList(maxWidth:));
- (LHDisplayList *)displayListWithMaxWidth:(CGFloat)maxWidth gpu:(LHGPURenderer *)renderer scale:(CGFloat)scale NS_SWIFT_NAME(displayList(maxWidth:gpu:scale:));
- (CGRect)boundsOfElementsMatching:(NSString *)selector NS_SWIFT_NAME(bounds(matching:));
- (void)setPaintExclusions:(NSSet<NSString *> *)values attribute:(NSString *)attribute NS_SWIFT_NAME(setPaintExclusions(_:attribute:));
- (nullable CGImageRef)copyImageOfElementWithAttribute:(NSString *)attribute value:(NSString *)value scale:(CGFloat)scale inkBounds:(CGRect *)inkBounds borderBox:(CGRect *)borderBox CF_RETURNS_RETAINED NS_SWIFT_NAME(copyImageOfElement(attribute:value:scale:inkBounds:borderBox:));
- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value isolatedInRect:(CGRect)rect NS_SWIFT_NAME(isElement(attribute:value:isolatedIn:));
- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value unobscuredInRect:(CGRect)rect NS_SWIFT_NAME(isElement(attribute:value:unobscuredIn:));
- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value unobscuredInRect:(CGRect)rect ignoring:(NSSet<NSString *> *)ignoring NS_SWIFT_NAME(isElement(attribute:value:unobscuredIn:ignoring:));
- (BOOL)isElementWithAttribute:(NSString *)attribute value:(NSString *)value unobscuredInRect:(CGRect)rect ignoring:(NSSet<NSString *> *)ignoring clipped:(BOOL)clipped NS_SWIFT_NAME(isElement(attribute:value:unobscuredIn:ignoring:clipped:));
- (BOOL)clipOfElementWithAttribute:(NSString *)attribute value:(NSString *)value rect:(CGRect *)rect radii:(CGFloat *)radii NS_SWIFT_NAME(clipOfElement(attribute:value:rect:radii:));
- (NSDictionary<NSString *, NSNumber *> *)paintOrderOfAttribute:(NSString *)attribute values:(NSSet<NSString *> *)values NS_SWIFT_NAME(paintOrder(attribute:values:));
- (nullable NSString *)attribute:(NSString *)name atPoint:(CGPoint)point NS_SWIFT_NAME(attribute(_:at:));
- (NSDictionary<NSString *, NSValue *> *)boundsByAttribute:(NSString *)name NS_SWIFT_NAME(bounds(byAttribute:));
- (NSDictionary<NSString *, NSString *> *)parentsByAttribute:(NSString *)name NS_SWIFT_NAME(parents(byAttribute:));
- (NSDictionary<NSString *, NSString *> *)computedStyleForAttribute:(NSString *)attribute value:(NSString *)value NS_SWIFT_NAME(computedStyle(attribute:value:));
- (NSDictionary<NSString *, NSDictionary<NSString *, NSString *> *> *)declarations:(NSArray<NSString *> *)names byAttribute:(NSString *)attribute NS_SWIFT_NAME(declarations(_:byAttribute:));
- (NSDictionary<NSString *, NSDictionary<NSString *, NSString *> *> *)declarations:(NSArray<NSString *> *)names byAttribute:(NSString *)attribute requiringAny:(NSArray<NSString *> *)required NS_SWIFT_NAME(declarations(_:byAttribute:requiringAny:));

- (NSArray<LHTextFragment *> *)textFragmentsWithMaxWidth:(CGFloat)maxWidth NS_SWIFT_NAME(textFragments(maxWidth:));

- (BOOL)mouseMoved:(CGPoint)point;
- (BOOL)mouseDown:(CGPoint)point;
- (BOOL)mouseUp:(CGPoint)point;
- (BOOL)mouseExited;

@property (readonly, copy) NSString *cursor;

@property (nonatomic, copy, nullable) void (^anchorHandler)(NSString *url);
@property (nonatomic, copy, nullable) void (^actionHandler)(NSString *action, NSString *_Nullable value);

@property (readonly) double parseMilliseconds;
@property (readonly) double layoutMilliseconds;

@property (readonly) NSDictionary<NSString *, NSNumber *> *sanitizerCounts;
@property (readonly) NSArray<NSString *> *sanitizerDetails;

@end

NS_ASSUME_NONNULL_END
