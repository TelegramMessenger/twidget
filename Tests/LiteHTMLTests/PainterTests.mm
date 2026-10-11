#import <XCTest/XCTest.h>
#import "LHDocument.h"
#import "LHSanitizer.h"
#import "LHDocumentPainting.h"

#include <string>
#include <vector>

namespace
{
struct RecordingPainter : lhx::Painter
{
    std::vector<std::string> texts;
    int                      fills = 0;
    int                      borders_drawn = 0;
    int                      clips = 0;

    void text(const char* text, const lhx::TextStyle& style, const litehtml::web_color& color, const litehtml::position& pos) override
    {
        texts.emplace_back(text);
    }
    void solidFill(const litehtml::background_layer& layer, const litehtml::web_color& color) override { fills++; }
    void borders(const litehtml::borders& b, const litehtml::position& box, bool root) override { borders_drawn++; }
    void pushClip(const litehtml::position& box, const litehtml::border_radiuses& radius) override { clips++; }
};
}

@interface PainterTests : XCTestCase
@end

@implementation PainterTests

- (void)testCustomPainterReceivesTheDocument
{
    LHDocument *document = [[LHDocument alloc] initWithHTML:@"<div style=\"background:#ff0000;border:1px solid #000;overflow:hidden\">Hello <b>world</b></div>" css:nil];
    RecordingPainter painter;
    CGSize size = [document paintWithMaxWidth:300 clip:CGRectMake(0, 0, 300, 300) painter:painter];
    XCTAssertGreaterThan(size.height, 0);
    XCTAssertGreaterThanOrEqual(painter.fills, 1);
    XCTAssertGreaterThanOrEqual(painter.borders_drawn, 1);
    XCTAssertGreaterThanOrEqual(painter.clips, 1);
    std::string joined;
    for(const auto &t : painter.texts)
        joined += t;
    XCTAssertTrue(joined.find("Hello") != std::string::npos);
    XCTAssertTrue(joined.find("world") != std::string::npos);
}

@end

@interface SanitizerTests : XCTestCase
@end

@implementation SanitizerTests

- (void)tearDown
{
    [LHSanitizer setLinkSchemes:@[@"http", @"https", @"mailto"] imageSchemes:@[]];
}

- (void)testDefaultPolicyDropsScriptsHandlersAndRemoteImages
{
    [LHSanitizer setLinkSchemes:@[@"http", @"https", @"mailto"] imageSchemes:@[]];
    LHDocument *document = [[LHDocument alloc] initWithHTML:@"<div onclick=\"x()\"><script>alert(1)</script><iframe src=\"https://a\"></iframe><img src=\"https://evil/p.gif\"><a href=\"javascript:alert(1)\">x</a></div>" css:nil];
    [document layoutWithMaxWidth:300];
    NSDictionary<NSString *, NSNumber *> *counts = document.sanitizerCounts;
    XCTAssertEqual(counts[@"attr-dropped"].intValue, 1);
    XCTAssertEqual(counts[@"tag-dropped"].intValue, 1);
    XCTAssertEqual(counts[@"url-blocked"].intValue, 2);
}

- (void)testHostSchemesAreConfigurable
{
    [LHSanitizer setLinkSchemes:@[@"https", @"myapp"] imageSchemes:@[@"asset"]];
    LHDocument *document = [[LHDocument alloc] initWithHTML:@"<a href=\"myapp:open\">a</a><img src=\"asset:logo\"><a href=\"http://x\">b</a>" css:nil];
    [document layoutWithMaxWidth:300];
    NSDictionary<NSString *, NSNumber *> *counts = document.sanitizerCounts;
    XCTAssertEqual(counts[@"url-blocked"].intValue, 1);
}

@end
