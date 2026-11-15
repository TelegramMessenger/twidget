#import <XCTest/XCTest.h>
#import <ImageIO/ImageIO.h>
#import "LHImages.h"

@interface IconPNGTests : XCTestCase
@end

@implementation IconPNGTests

- (NSData*)payload:(NSString*)url
{
    NSRange comma = [url rangeOfString:@","];
    return [[NSData alloc] initWithBase64EncodedString:[url substringFromIndex:comma.location + 1] options:0];
}

- (void)testAnIconBecomesARetinaPNGThatKeepsItsCSSSize
{
    NSString* url = [LHImages iconPNGDataURL:@"icon:star?size=24&color=ff0000"];
    XCTAssertNotNil(url);
    XCTAssertTrue([url hasPrefix:@"data:image/png;base64,"]);
    NSData* png = [self payload:url];
    const uint8_t magic[8] = {0x89, 'P', 'N', 'G', 0x0D, 0x0A, 0x1A, 0x0A};
    XCTAssertEqual(memcmp(png.bytes, magic, 8), 0);
    CGImageSourceRef src = CGImageSourceCreateWithData((__bridge CFDataRef)png, nullptr);
    NSDictionary* props = CFBridgingRelease(CGImageSourceCopyPropertiesAtIndex(src, 0, nullptr));
    CFRelease(src);
    XCTAssertEqual([props[(__bridge id)kCGImagePropertyPixelWidth] intValue], 48);
    XCTAssertEqual([props[(__bridge id)kCGImagePropertyDPIWidth] intValue], 144);
    CGSize size = [LHImages sizeForURL:url];
    XCTAssertEqualWithAccuracy(size.width, 24, 0.01);
    XCTAssertEqualWithAccuracy(size.height, 24, 0.01);
    CGImageRef image = [LHImages copyImageForURL:url];
    XCTAssertNotNil((__bridge id)image);
    XCTAssertEqual(CGImageGetWidth(image), 48u);
    CGImageRelease(image);
}

- (void)testTheLargestIconStaysWithinTheDecodeCap
{
    NSString* url = [LHImages iconPNGDataURL:@"icon:star?size=512"];
    XCTAssertNotNil(url);
    CGImageRef image = [LHImages copyImageForURL:url];
    XCTAssertNotNil((__bridge id)image);
    XCTAssertLessThanOrEqual(CGImageGetWidth(image), 1024u);
    CGImageRelease(image);
}

- (void)testUnknownOrMalformedIconsGiveNothing
{
    XCTAssertNil([LHImages iconPNGDataURL:@"sf:star"]);
    XCTAssertNil([LHImages iconPNGDataURL:@"https://example.com/a.png"]);
    XCTAssertNil([LHImages iconPNGDataURL:[@"icon:" stringByPaddingToLength:600 withString:@"a" startingAtIndex:0]]);
}

- (void)testARetinaDPIAloneDoesNotHalveAWidgetImage
{
    NSString* url = [LHImages iconPNGDataURL:@"icon:star?size=10"];
    NSData* png = [self payload:url];
    CGImageSourceRef src = CGImageSourceCreateWithData((__bridge CFDataRef)png, nullptr);
    CGImageRef image = CGImageSourceCreateImageAtIndex(src, 0, nullptr);
    CFRelease(src);
    NSMutableData* plain = [NSMutableData data];
    CGImageDestinationRef dest = CGImageDestinationCreateWithData((__bridge CFMutableDataRef)plain, CFSTR("public.png"), 1, nullptr);
    CGImageDestinationAddImage(dest, image, (__bridge CFDictionaryRef)@{(__bridge id)kCGImagePropertyDPIWidth: @144, (__bridge id)kCGImagePropertyDPIHeight: @144});
    CGImageDestinationFinalize(dest);
    CFRelease(dest);
    CGImageRelease(image);
    NSString* plainURL = [@"data:image/png;base64," stringByAppendingString:[plain base64EncodedStringWithOptions:0]];
    XCTAssertEqualWithAccuracy([LHImages sizeForURL:plainURL].width, 20, 0.01);
}

- (void)testAnOrdinaryPNGKeepsItsPixelSize
{
    NSString* url = [LHImages iconPNGDataURL:@"icon:star?size=10"];
    NSData* png = [self payload:url];
    CGImageSourceRef src = CGImageSourceCreateWithData((__bridge CFDataRef)png, nullptr);
    CGImageRef image = CGImageSourceCreateImageAtIndex(src, 0, nullptr);
    CFRelease(src);
    NSMutableData* plain = [NSMutableData data];
    CGImageDestinationRef dest = CGImageDestinationCreateWithData((__bridge CFMutableDataRef)plain, CFSTR("public.png"), 1, nullptr);
    CGImageDestinationAddImage(dest, image, nullptr);
    CGImageDestinationFinalize(dest);
    CFRelease(dest);
    CGImageRelease(image);
    NSString* plainURL = [@"data:image/png;base64," stringByAppendingString:[plain base64EncodedStringWithOptions:0]];
    CGSize size = [LHImages sizeForURL:plainURL];
    XCTAssertEqualWithAccuracy(size.width, 20, 0.01);
}

@end
