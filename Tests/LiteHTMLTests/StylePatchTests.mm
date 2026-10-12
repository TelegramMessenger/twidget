#import <XCTest/XCTest.h>
#import "LHDocument.h"

#include <random>

@interface StylePatchTests : XCTestCase
@end

@implementation StylePatchTests

static NSString *const Attribute = @"data-lh-id";

static NSString *Page(NSString *a, NSString *b, NSString *fill)
{
    return [NSString stringWithFormat:@"<!DOCTYPE html><div data-lh-id=\"1\" style=\"padding:8px;%@\">"
                                       "<div data-lh-id=\"2\" style=\"%@\">Hello <b>world</b></div>"
                                       "<svg data-lh-id=\"3\" width=\"40\" height=\"20\" viewBox=\"0 0 40 20\"><circle data-lh-id=\"4\" cx=\"10\" cy=\"10\" r=\"8\" style=\"%@\"/></svg>"
                                       "<span data-lh-id=\"5\">inline</span>"
                                       "<div data-lh-id=\"6\" style=\"transform:rotate(5deg)\"><div data-lh-id=\"7\" style=\"height:10px;background-color:red;\">x</div></div>"
                                       "<p data-lh-id=\"8\">plain</p></div>",
                                       a, b, fill];
}

static NSData *Pixels(LHDocument *document)
{
    CGSize     size  = [document layoutWithMaxWidth:240 preferredWidth:240];
    CGImageRef image = [document copyImageWithMaxWidth:240 rect:CGRectMake(0, 0, 240, ceil(size.height)) scale:2];
    if(!image)
        return [NSData data];
    NSData *data = (__bridge_transfer NSData *)CGDataProviderCopyData(CGImageGetDataProvider(image));
    CGImageRelease(image);
    return data;
}

static LHDocument *Laid(NSString *html)
{
    LHDocument *document = [[LHDocument alloc] initWithHTML:html css:nil previous:nil];
    [document layoutWithMaxWidth:240 preferredWidth:240];
    return document;
}

- (void)testStylePatchMatchesAnHTMLPatch
{
    LHDocument *previous = Laid(Page(@"background-color:#fff;", @"color:#111;", @"fill:#00f;"));
    LHDocument *patched  = [[LHDocument alloc] initWithPrevious:previous styles:@{@"1": @"padding:8px;background-color:#eef;", @"2": @"color:#a00;font-weight:bold;", @"4": @"fill:#0a0;"} attribute:Attribute css:nil];
    XCTAssertNotNil(patched);
    XCTAssertTrue(patched.reusedPrevious);
    NSString *expected = Page(@"background-color:#eef;", @"color:#a00;font-weight:bold;", @"fill:#0a0;");
    XCTAssertEqualObjects(patched.html, expected);
    XCTAssertEqualObjects(Pixels(patched), Pixels(Laid(expected)));
    XCTAssertEqualObjects(Pixels(previous), Pixels(Laid(Page(@"background-color:#fff;", @"color:#111;", @"fill:#00f;"))));
}

- (void)testStylePatchRefusesWhatHTMLCouldNotCarry
{
    NSString   *html     = Page(@"background-color:#fff;", @"color:#111;", @"fill:#00f;");
    LHDocument *previous = Laid(html);
    NSArray<NSDictionary<NSString *, NSString *> *> *refused = @[
        @{},
        @{@"5": @"color:red;"},
        @{@"99": @"color:red;"},
        @{@"2": @"color:red;\" onclick=\"x"},
        @{@"2": @"color:red;<script>"},
        @{@"2": @"color:&quot;red"},
        @{@"2": @"--x:1;"},
        @{@"2": @"counter-reset:a;"},
    ];
    for(NSDictionary *styles in refused)
    {
        XCTAssertNil([[LHDocument alloc] initWithPrevious:previous styles:styles attribute:Attribute css:nil], @"%@", styles);
        XCTAssertEqualObjects(previous.html, html);
    }
    XCTAssertEqualObjects(Pixels(previous), Pixels(Laid(html)));
    XCTAssertNotNil([[LHDocument alloc] initWithPrevious:previous styles:@{@"2": @"color:red;"} attribute:Attribute css:nil]);
}

- (void)testPaintOnlyPatchesNameTheirRegions
{
    NSString *base = Page(@"background-color:#fff;", @"color:#111;", @"fill:#00f;");
    struct Case
    {
        NSDictionary<NSString *, NSString *> *styles;
        int                                   rects;
    };
    const Case cases[] = {
        {@{@"1": @"padding:8px;background-color:#eef;"}, 1},
        {@{@"2": @"color:#a00;"}, -1},
        {@{@"2": @"color:#111;transform:scale(2);"}, -1},
        {@{@"2": @"color:#111;opacity:0.5;"}, -1},
        {@{@"4": @"fill:#0a0;"}, 1},
        {@{@"7": @"height:10px;background-color:blue;"}, -1},
        {@{@"2": @"color:#111;"}, 0},
        {@{@"1": @"padding:9px;background-color:#fff;"}, -1},
    };
    for(const Case &c : cases)
    {
        LHDocument *previous = Laid(base);
        LHDocument *patched  = [[LHDocument alloc] initWithPrevious:previous styles:c.styles attribute:Attribute css:nil];
        XCTAssertNotNil(patched, @"%@", c.styles);
        NSArray<NSValue *> *rects = patched.patchDirtyRects;
        if(c.rects < 0)
        {
            XCTAssertNil(rects, @"%@", c.styles);
            continue;
        }
        XCTAssertEqual((int)rects.count, c.rects, @"%@", c.styles);
        for(NSValue *value in rects)
        {
            NSString *key = c.styles.allKeys.firstObject;
            NSString *box = [key isEqualToString:@"4"] ? @"3" : key;
            CGRect    want = [[patched boundsByAttribute:Attribute][box] rectValue];
            XCTAssertTrue(CGRectContainsRect(value.rectValue, want), @"%@ %@ %@", c.styles, NSStringFromRect(value.rectValue), NSStringFromRect(want));
        }
    }
}

- (void)testRegionsCoverEveryChangedPixel
{
    NSString   *base     = Page(@"background-color:#fff;", @"color:#111;", @"fill:#00f;");
    LHDocument *previous = Laid(base);
    NSData     *before   = Pixels(previous);
    LHDocument *patched  = [[LHDocument alloc] initWithPrevious:previous styles:@{@"1": @"padding:8px;background-color:#eef;", @"4": @"fill:#0a0;"} attribute:Attribute css:nil];
    NSArray<NSValue *> *rects = patched.patchDirtyRects;
    XCTAssertNotNil(rects);
    NSData *after = Pixels(patched);
    XCTAssertEqual(before.length, after.length);
    XCTAssertNotEqualObjects(before, after);
    CGSize size = patched.size;
    size_t width = 480, rows = before.length / (width * 4);
    const uint32_t *a = (const uint32_t *)before.bytes, *b = (const uint32_t *)after.bytes;
    for(size_t y = 0; y < rows; y++)
    {
        for(size_t x = 0; x < width; x++)
        {
            if(a[y * width + x] == b[y * width + x])
                continue;
            CGPoint point = CGPointMake((x + 0.5) / 2, (y + 0.5) / 2);
            BOOL    inside = NO;
            for(NSValue *value in rects)
                inside = inside || CGRectContainsPoint(value.rectValue, point);
            XCTAssertTrue(inside, @"pixel %zu,%zu changed outside the regions (height %.0f)", x, y, size.height);
            if(!inside)
                return;
        }
    }
}

- (void)testDeclarationsSkipUndisplayedElements
{
    LHDocument *document = Laid(@"<style>@keyframes k{to{opacity:0}}.a{animation:k 1s}</style>"
                                 "<div data-lh-id=\"1\" class=\"a\">shown</div>"
                                 "<div data-lh-id=\"2\" class=\"a\" hidden>hidden</div>"
                                 "<div style=\"display:none\"><div data-lh-id=\"3\" class=\"a\">inside</div></div>");
    NSDictionary *found = [document declarations:@[@"animation"] byAttribute:Attribute requiringAny:@[@"animation"]];
    XCTAssertNotNil(found[@"1"]);
    XCTAssertNil(found[@"2"]);
    XCTAssertNil(found[@"3"]);
}

- (void)testRandomStylePatchesMatchAFreshParse
{
    std::mt19937 random(7);
    NSArray<NSString *> *values = @[@"background-color:#fff;", @"background-color:rgba(0,0,0,0.3);", @"color:#a00;", @"padding:8px;", @"padding:8px;background-color:#eef;",
                                    @"opacity:0.4;", @"transform:translateX(4px);", @"fill:#0a0;", @"border:2px solid red;", @"display:none;", @"width:50%;", @"background:linear-gradient(red,blue);"];
    NSArray<NSString *> *ids = @[@"1", @"2", @"4"];
    LHDocument *document = Laid(Page(@"background-color:#fff;", @"color:#111;", @"fill:#00f;"));
    int applied = 0;
    for(int round = 0; round < 120; round++)
    {
        NSMutableDictionary<NSString *, NSString *> *styles = [NSMutableDictionary dictionary];
        for(NSString *key in ids)
        {
            if(random() % 2)
                styles[key] = values[random() % values.count];
        }
        LHDocument *next = [[LHDocument alloc] initWithPrevious:document styles:styles attribute:Attribute css:nil];
        if(!next)
            continue;
        applied++;
        XCTAssertEqualObjects(Pixels(next), Pixels(Laid(next.html)), @"round %d %@", round, styles);
        document = next;
    }
    XCTAssertGreaterThan(applied, 40);
}

@end
