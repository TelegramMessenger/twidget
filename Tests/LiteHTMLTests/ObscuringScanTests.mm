#import <XCTest/XCTest.h>
#import "LHDocument.h"

#include <initializer_list>

@interface ObscuringScanTests : XCTestCase
@end

@implementation ObscuringScanTests

static uint64_t Mix(uint64_t h, NSString *s)
{
    for(NSUInteger i = 0; i < s.length; i++)
        h = (h ^ [s characterAtIndex:i]) * 1099511628211ULL;
    return (h ^ 0xff) * 1099511628211ULL;
}

static NSString *Html(uint32_t &seed, int count)
{
    auto next = [&seed](uint32_t mod) { seed = seed * 1664525u + 1013904223u; return (seed >> 8) % mod; };
    NSMutableString *html = [NSMutableString stringWithString:@"<html><body style=\"margin:0\">"];
    int open = 0;
    for(int i = 1; i <= count; i++)
    {
        NSString *extra = @"";
        switch(next(8))
        {
        case 0: extra = @"opacity:0;"; break;
        case 1: extra = @"visibility:hidden;"; break;
        case 2: extra = @"overflow:hidden;"; break;
        default: break;
        }
        [html appendFormat:@"<div data-lh-id=\"%d\" style=\"position:absolute;left:%upx;top:%upx;width:%upx;height:%upx;background:#%06x;%@\">", i, next(200), next(200), 20 + next(80), 20 + next(80), next(0xffffff), extra];
        if(next(3) == 0)
            [html appendString:@"<span>text</span>"];
        if(next(6) == 0 && open < 2)
            open++;
        else
            [html appendString:@"</div>"];
        if(open > 0 && next(4) == 0)
        {
            [html appendString:@"</div>"];
            open--;
        }
    }
    while(open-- > 0)
        [html appendString:@"</div>"];
    [html appendString:@"</body></html>"];
    return html;
}

static uint64_t Signature(void)
{
    uint64_t h = 1469598103934665603ULL;
    uint32_t seed = 7;
    for(int doc = 0; doc < 12; doc++)
    {
        const int count = 40 + doc * 5;
        LHDocument *document = [[LHDocument alloc] initWithHTML:Html(seed, count) css:nil];
        [document layoutWithMaxWidth:400 preferredWidth:400];
        NSMutableSet<NSString *> *mutating = [NSMutableSet set];
        NSSet<NSString *> *fixed = [NSSet setWithObjects:@"3", @"5", @"8", @"13", @"21", nil];
        for(int i = 1; i <= count; i++)
        {
            NSString *value = [NSString stringWithFormat:@"%d", i];
            for(CGFloat margin : {0.0, 4.0, 40.0, 200.0})
            {
                CGRect rect = CGRectMake((i * 37) % 300 - margin, (i * 53) % 300 - margin, 40 + 2 * margin, 30 + 2 * margin);
                for(NSSet<NSString *> *ignoring : @[[NSSet set], fixed, mutating])
                {
                    for(BOOL clipped : {NO, YES})
                    {
                        NSArray<NSString *> *found = [document elementsObscuringAttribute:@"data-lh-id" value:value inRect:rect ignoring:ignoring clipped:clipped];
                        h = Mix(h, found ? [found componentsJoinedByString:@","] : @"nil");
                        BOOL unobscured = [document isElementWithAttribute:@"data-lh-id" value:value unobscuredInRect:rect ignoring:ignoring clipped:clipped];
                        h = Mix(h, unobscured ? @"y" : @"n");
                    }
                }
            }
            [mutating addObject:[NSString stringWithFormat:@"%d", (i * 7) % count + 1]];
        }
    }
    return h;
}

- (void)testObscuringResultsMatchReference
{
    XCTAssertEqual(Signature(), 6961146370578556114ULL);
}

- (void)testIgnoringSetVariantMatches
{
    uint32_t seed = 11;
    LHDocument *document = [[LHDocument alloc] initWithHTML:Html(seed, 80) css:nil];
    [document layoutWithMaxWidth:400 preferredWidth:400];
    NSMutableSet<NSString *> *mutating = [NSMutableSet set];
    NSSet<NSString *> *fixed = [NSSet setWithObjects:@"2", @"4", @"9", nil];
    NSUInteger nonEmpty = 0, missing = 0;
    for(int i = 1; i <= 80; i++)
    {
        NSString *value = [NSString stringWithFormat:@"%d", i];
        CGRect rect = CGRectMake((i * 37) % 300 - 20, (i * 53) % 300 - 20, 80, 70);
        for(NSSet<NSString *> *ignoring : @[fixed, mutating, fixed])
        {
            NSArray<NSString *> *a = [document elementsObscuringAttribute:@"data-lh-id" value:value inRect:rect ignoring:[ignoring copy] clipped:NO];
            NSArray<NSString *> *b = [document elementsObscuringAttribute:@"data-lh-id" value:value inRect:rect ignoringSet:ignoring clipped:NO];
            XCTAssertEqualObjects(a, b);
            XCTAssertEqual([document isElementWithAttribute:@"data-lh-id" value:value unobscuredInRect:rect ignoringSet:ignoring clipped:NO], a != nil && a.count == 0);
            nonEmpty += a.count > 0;
            missing += a == nil;
        }
        [mutating addObject:[NSString stringWithFormat:@"%d", (i * 7) % 80 + 1]];
    }
    XCTAssertGreaterThan(nonEmpty, 0u);
    XCTAssertLessThan(missing, 240u);
}

@end
