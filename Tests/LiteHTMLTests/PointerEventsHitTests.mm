#import <XCTest/XCTest.h>
#import "LHDocument.h"

@interface PointerEventsHitTests : XCTestCase
@end

@implementation PointerEventsHitTests

static NSString *Hit(NSString *html, CGPoint point)
{
    LHDocument *document = [[LHDocument alloc] initWithHTML:html css:nil];
    [document layoutWithMaxWidth:420 preferredWidth:420];
    return [document attribute:@"data-id" atPoint:point];
}

- (void)testOverlayWithoutPointerEventsPassesClicksThrough
{
    NSString *html = @"<div style='position:relative;height:150px' data-id='wrap'><a href='https://example.com' data-id='link' style='display:block;height:150px'>link</a><div data-id='overlay' style='position:absolute;left:0;top:0;width:420px;height:150px;pointer-events:none'></div></div>";
    XCTAssertEqualObjects(Hit(html, CGPointMake(40, 40)), @"link");
}

- (void)testOverlayWithPointerEventsStillCatchesClicks
{
    NSString *html = @"<div style='position:relative;height:150px' data-id='wrap'><a href='https://example.com' data-id='link' style='display:block;height:150px'>link</a><div data-id='overlay' style='position:absolute;left:0;top:0;width:420px;height:150px'></div></div>";
    XCTAssertEqualObjects(Hit(html, CGPointMake(40, 40)), @"overlay");
}

- (void)testInheritedNoneCanBeTurnedBackOnByAChild
{
    NSString *html = @"<div style='pointer-events:none;height:150px' data-id='outer'><div data-id='inner' style='pointer-events:auto;height:60px'>x</div><div data-id='dead' style='height:60px'>y</div></div>";
    XCTAssertEqualObjects(Hit(html, CGPointMake(40, 20)), @"inner");
    XCTAssertNotEqualObjects(Hit(html, CGPointMake(40, 90)), @"dead");
    XCTAssertNotEqualObjects(Hit(html, CGPointMake(40, 90)), @"outer");
}

@end
