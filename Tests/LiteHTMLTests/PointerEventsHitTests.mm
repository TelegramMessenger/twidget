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

- (void)testSVGAnchorShapesAreHitTargets
{
    NSString *html = @"<svg data-id='svg' width='400' height='150' viewBox='0 0 200 75'><rect data-id='bg' width='200' height='75' fill='#eee'/><a href='https://example.com' data-id='link'><g transform='translate(100,0)'><rect data-id='shape' x='10' y='10' width='50' height='30' fill='blue'/></g></a></svg>";
    XCTAssertEqualObjects(Hit(html, CGPointMake(230, 40)), @"shape");
    XCTAssertEqualObjects(Hit(html, CGPointMake(20, 20)), @"bg");
    XCTAssertEqualObjects(Hit(html, CGPointMake(380, 140)), @"bg");
}

- (void)testSVGLaterShapesWinAndEmptyAreasHitTheSVG
{
    NSString *html = @"<svg data-id='svg' width='300' height='100'><circle data-id='under' cx='50' cy='50' r='40'/><a href='https://example.com' data-id='link'><circle data-id='over' cx='60' cy='50' r='20'/></a></svg>";
    XCTAssertEqualObjects(Hit(html, CGPointMake(60, 50)), @"over");
    XCTAssertEqualObjects(Hit(html, CGPointMake(20, 50)), @"under");
    XCTAssertEqualObjects(Hit(html, CGPointMake(250, 50)), @"svg");
}

@end
